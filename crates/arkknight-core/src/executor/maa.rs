//! MaaCliExecutor（§8.2）：MAA 任务会话 + 切号（switch 模块直接调 maa，不经此层）。
//!
//! 每账号独立 `MAA_CONFIG_DIR = <workdir>/accounts/<key>/maa/`（profiles + tasks），
//! 完全隔离；运行 = `maa run <task> -p default --batch` 子进程。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use tokio::process::Child;

use crate::executor::{Executor, ExecutorError, ExecutorHandle, ExecutorHealth, SessionCtx};
use crate::model::ExecutorKind;

pub struct MaaCliExecutor {
    maa_bin: PathBuf,
    /// session_id → 子进程（drain/health 需要按句柄找到进程）
    children: Mutex<HashMap<u64, Child>>,
}

impl MaaCliExecutor {
    pub fn new(maa_bin: impl Into<PathBuf>) -> Self {
        MaaCliExecutor {
            maa_bin: maa_bin.into(),
            children: Mutex::new(HashMap::new()),
        }
    }

    fn take_child(&self, session_id: u64) -> Result<Child, ExecutorError> {
        self.children
            .lock()
            .expect("children 锁 poisoned")
            .remove(&session_id)
            .ok_or_else(|| ExecutorError::Drain(format!("会话 {session_id} 无运行中的 maa 子进程")))
    }
}

#[async_trait::async_trait]
impl Executor for MaaCliExecutor {
    fn kind(&self) -> ExecutorKind {
        ExecutorKind::Maa
    }

    async fn start(&self, ctx: &SessionCtx) -> Result<ExecutorHandle, ExecutorError> {
        let task = ctx.maa_task.as_deref().ok_or_else(|| {
            ExecutorError::Start("MAA 会话须指定任务名（SessionCtx.maa_task）".into())
        })?;
        let config_dir = ctx
            .workdir
            .join("accounts")
            .join(&ctx.account.key)
            .join("maa");
        if !config_dir.is_dir() {
            return Err(ExecutorError::Start(format!(
                "MAA 配置目录不存在：{}（先运行 switch 或 provision 完成物化）",
                config_dir.display()
            )));
        }
        let child = tokio::process::Command::new(&self.maa_bin)
            .args(["run", task, "-p", "default", "--batch"])
            .env("MAA_CONFIG_DIR", &config_dir)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| {
                ExecutorError::Start(format!("spawn {} 失败：{e}", self.maa_bin.display()))
            })?;
        let locator = format!("pid={}", child.id().unwrap_or(0));
        self.children
            .lock()
            .expect("children 锁 poisoned")
            .insert(ctx.session_id, child);
        Ok(ExecutorHandle {
            session_id: ctx.session_id,
            kind: self.kind(),
            runner: ctx.runner,
            locator,
        })
    }

    async fn drain(&self, handle: &ExecutorHandle, grace: Duration) -> Result<(), ExecutorError> {
        let mut child = self.take_child(handle.session_id)?;
        // maa-cli 无优雅停止 API：给 grace 时间自然结束（任务收尾），超时强杀
        match tokio::time::timeout(grace, child.wait()).await {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(e)) => Err(ExecutorError::Drain(format!("等待 maa 退出失败：{e}"))),
            Err(_) => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                Ok(())
            }
        }
    }

    async fn health(&self, handle: &ExecutorHandle) -> ExecutorHealth {
        let mut map = self.children.lock().expect("children 锁 poisoned");
        match map.get_mut(&handle.session_id) {
            Some(child) => match child.try_wait() {
                Ok(None) => ExecutorHealth::Alive,
                Ok(Some(status)) => {
                    tracing::info!(session = handle.session_id, %status, "maa 子进程已退出");
                    ExecutorHealth::Dead
                }
                Err(e) => {
                    tracing::warn!(session = handle.session_id, "探测 maa 子进程失败：{e}");
                    ExecutorHealth::Unknown
                }
            },
            None => ExecutorHealth::Dead,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::DeviceEndpoints;
    use crate::model::{Account, RunnerKind, Server};

    fn ctx(dir: &std::path::Path, task: Option<&str>) -> SessionCtx {
        SessionCtx {
            session_id: 1,
            account: Account {
                key: "main".into(),
                display_name: "m".into(),
                server: Server::Official,
                account_name: "1***2".into(),
                uid: None,
                enabled: true,
                schedule: Default::default(),
                provisioned_on: vec![],
            },
            endpoints: DeviceEndpoints {
                host_adb: "127.0.0.1:1".into(),
                docker_adb: None,
                docker_network: None,
            },
            runner: RunnerKind::Process,
            mower_port: None,
            workdir: dir.to_path_buf(),
            mower_checkout: dir.to_path_buf(),
            maa_task: task.map(String::from),
        }
    }

    fn fake_maa(dir: &std::path::Path) -> PathBuf {
        // 跨平台假 maa：启动后挂住，收到 SIGTERM/kill 才退出
        let path = dir.join(if cfg!(windows) {
            "fake-maa.bat"
        } else {
            "fake-maa.sh"
        });
        let script = if cfg!(windows) {
            "@echo off\r\nping -n 61 127.0.0.1 > nul\r\n".to_string()
        } else {
            "#!/bin/sh\nsleep 60\n".to_string()
        };
        std::fs::write(&path, script).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        path
    }

    #[tokio::test]
    async fn start_requires_task_and_existing_config_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let ex = MaaCliExecutor::new("maa");
        // 未指定任务
        assert!(ex.start(&ctx(tmp.path(), None)).await.is_err());
        // 目录不存在
        assert!(ex.start(&ctx(tmp.path(), Some("startup"))).await.is_err());
    }

    #[tokio::test]
    async fn spawn_drain_and_health_with_fake_maa() {
        let tmp = tempfile::tempdir().unwrap();
        let config_dir = tmp.path().join("accounts/main/maa");
        std::fs::create_dir_all(&config_dir).unwrap();

        let ex = MaaCliExecutor::new(fake_maa(tmp.path()));
        let handle = ex.start(&ctx(tmp.path(), Some("startup"))).await.unwrap();
        assert!(handle.locator.starts_with("pid="));
        // 高并行负载下子进程 fork 可能延迟一拍，重试一小段时间再断言存活
        let mut alive = ExecutorHealth::Dead;
        for _ in 0..10 {
            alive = ex.health(&handle).await;
            if alive == ExecutorHealth::Alive {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert_eq!(alive, ExecutorHealth::Alive);

        // grace 1s 内不会自然结束 → 强杀路径
        ex.drain(&handle, Duration::from_secs(1)).await.unwrap();
        assert_eq!(ex.health(&handle).await, ExecutorHealth::Dead);
    }

    #[tokio::test]
    async fn drain_without_child_errors() {
        let ex = MaaCliExecutor::new("maa");
        let h = ExecutorHandle {
            session_id: 42,
            kind: ExecutorKind::Maa,
            runner: RunnerKind::Process,
            locator: "pid=0".into(),
        };
        assert!(ex.drain(&h, Duration::from_millis(10)).await.is_err());
        assert_eq!(ex.health(&h).await, ExecutorHealth::Dead);
    }
}
