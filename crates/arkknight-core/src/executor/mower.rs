//! MowerExecutor 的 ProcessRunner 形态（§8.3，ADR-0001 D5）。
//!
//! 本地 mower 检出 + Python 环境：spawn `<python> <data>/run_server.py`，
//! `MOWER_DATA_DIR = accounts/<key>/mower-data`（config 符号链接直指 bundle，
//! 用户在 mower UI 的改动天然持久化，§11.4）。
//! DockerRunner（bollard）形态属任务 6 后半交付。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Child;

use crate::executor::{Executor, ExecutorError, ExecutorHandle, ExecutorHealth, SessionCtx};
use crate::materialize::{ensure_process_data_dir, render_process_launcher};
use crate::model::{ExecutorKind, RunnerKind};

/// 解析 locator（`pid=<n> port=<p>`）。
pub fn parse_locator(locator: &str) -> (Option<u32>, Option<u16>) {
    let mut pid = None;
    let mut port = None;
    for part in locator.split_whitespace() {
        if let Some(v) = part.strip_prefix("pid=") {
            pid = v.parse().ok();
        } else if let Some(v) = part.strip_prefix("port=") {
            port = v.parse().ok();
        }
    }
    (pid, port)
}

/// 优雅停止请求：`POST /stop?token=<t>`（mower 自带端点，§3.1）。
pub async fn stop_via_http(port: u16, token: &str) -> Result<(), std::io::Error> {
    let req = format!(
        "POST /stop?token={token} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\nContent-Length: 0\r\n\r\n"
    );
    let stream = tokio::time::timeout(
        Duration::from_secs(5),
        tokio::net::TcpStream::connect(("127.0.0.1", port)),
    )
    .await
    .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "连接 mower 超时"))??;
    let mut stream = stream;
    tokio::time::timeout(Duration::from_secs(5), async move {
        stream.write_all(req.as_bytes()).await?;
        let mut buf = Vec::new();
        let _ = stream.read_to_end(&mut buf).await;
        Ok::<_, std::io::Error>(())
    })
    .await
    .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "/stop 响应超时"))??;
    Ok(())
}

pub struct MowerProcessExecutor {
    /// python 可执行文件（默认 `python3`，Windows 回退 `python` 由调用方决定）
    python_bin: PathBuf,
    children: Mutex<HashMap<u64, Child>>,
}

impl MowerProcessExecutor {
    pub fn new(python_bin: impl Into<PathBuf>) -> Self {
        MowerProcessExecutor {
            python_bin: python_bin.into(),
            children: Mutex::new(HashMap::new()),
        }
    }

    fn take_child(&self, session_id: u64) -> Result<Child, ExecutorError> {
        self.children
            .lock()
            .expect("children 锁 poisoned")
            .remove(&session_id)
            .ok_or_else(|| {
                ExecutorError::Drain(format!("会话 {session_id} 无运行中的 mower 子进程"))
            })
    }
}

#[async_trait::async_trait]
impl Executor for MowerProcessExecutor {
    fn kind(&self) -> ExecutorKind {
        ExecutorKind::Mower
    }

    async fn start(&self, ctx: &SessionCtx) -> Result<ExecutorHandle, ExecutorError> {
        let port = ctx.mower_port.ok_or_else(|| {
            ExecutorError::Start("mower 会话须分配 webview 端口（SessionCtx.mower_port）".into())
        })?;
        let account_dir = ctx.workdir.join("accounts").join(&ctx.account.key);
        let bundle = account_dir.join("mower");
        let conf = bundle.join("conf.yml");
        if !conf.is_file() {
            return Err(ExecutorError::Start(format!(
                "账号 bundle 缺少 {}（从现有部署导入或手工放置 conf.yml/plan.json）",
                conf.display()
            )));
        }
        let data_dir = ensure_process_data_dir(&account_dir)
            .map_err(|e| ExecutorError::Start(e.to_string()))?;

        // 会话启动瞬间：白名单改写 conf.yml（§11.4 时序约束）
        let raw = std::fs::read_to_string(&conf)
            .map_err(|e| ExecutorError::Start(format!("读取 {} 失败：{e}", conf.display())))?;
        let patch = crate::materialize::MowerPatch {
            adb: ctx.endpoints.host_adb.clone(),
            port,
            token: Some(ctx.webview_token()),
            start_automatically: true,
        };
        let patched = crate::materialize::patch_conf(&raw, &patch)
            .map_err(|e| ExecutorError::Start(e.to_string()))?;
        std::fs::write(&conf, patched)
            .map_err(|e| ExecutorError::Start(format!("写回 {} 失败：{e}", conf.display())))?;

        // 启动器（arkknight 拥有，幂等重写）+ 日志归档（§14）
        let launcher = data_dir.join("run_server.py");
        std::fs::write(&launcher, render_process_launcher(&ctx.mower_checkout))
            .map_err(|e| ExecutorError::Start(format!("写启动器失败：{e}")))?;
        let log_path = ctx
            .workdir
            .join("logs")
            .join("sessions")
            .join(format!("{}.log", ctx.session_id));
        if let Some(dir) = log_path.parent() {
            std::fs::create_dir_all(dir)
                .map_err(|e| ExecutorError::Start(format!("建日志目录失败：{e}")))?;
        }
        let log_file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .map_err(|e| ExecutorError::Start(format!("打开会话日志失败：{e}")))?;

        let child = tokio::process::Command::new(&self.python_bin)
            .arg(&launcher)
            .current_dir(&ctx.mower_checkout)
            .env("MOWER_DATA_DIR", &data_dir)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::from(log_file.try_clone().map_err(|e| {
                ExecutorError::Start(format!("复制日志句柄失败：{e}"))
            })?))
            .stderr(std::process::Stdio::from(log_file))
            .spawn()
            .map_err(|e| {
                ExecutorError::Start(format!(
                    "spawn {} 失败：{e}（ProcessRunner 需 Python 3.11+ 与 mower 依赖，doctor 可检测）",
                    self.python_bin.display()
                ))
            })?;
        let locator = format!("pid={} port={port}", child.id().unwrap_or(0));
        self.children
            .lock()
            .expect("children 锁 poisoned")
            .insert(ctx.session_id, child);

        Ok(ExecutorHandle {
            session_id: ctx.session_id,
            kind: self.kind(),
            runner: RunnerKind::Process,
            locator,
        })
    }

    async fn drain(&self, handle: &ExecutorHandle, grace: Duration) -> Result<(), ExecutorError> {
        let mut child = self.take_child(handle.session_id)?;
        // 优雅通道：POST /stop（mower 自带端点）；失败不阻断，超时后强杀兜底
        let (_, port) = parse_locator(&handle.locator);
        if let Some(port) = port {
            let token = crate::executor::webview_token_for(handle.session_id);
            if let Err(e) = stop_via_http(port, &token).await {
                tracing::warn!(
                    session = handle.session_id,
                    "POST /stop 失败（将走超时强杀）：{e}"
                );
            }
        }
        match tokio::time::timeout(grace, child.wait()).await {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(e)) => Err(ExecutorError::Drain(format!("等待 mower 退出失败：{e}"))),
            Err(_) => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                tracing::warn!(session = handle.session_id, "mower 优雅停止超时，已强杀");
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
                    tracing::info!(session = handle.session_id, %status, "mower 子进程已退出");
                    ExecutorHealth::Dead
                }
                Err(_) => ExecutorHealth::Unknown,
            },
            None => ExecutorHealth::Dead,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::DeviceEndpoints;
    use crate::model::{Account, Server};

    fn ctx(dir: &std::path::Path) -> SessionCtx {
        SessionCtx {
            session_id: 7,
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
                host_adb: "127.0.0.1:2771".into(),
                docker_adb: None,
                docker_network: None,
            },
            runner: RunnerKind::Process,
            mower_port: Some(58110),
            workdir: dir.to_path_buf(),
            mower_checkout: dir.to_path_buf(),
            maa_task: None,
        }
    }

    /// 假 python：回显参数并挂住；用于验证启动参数与日志归档，不依赖真实 mower。
    fn fake_python(dir: &std::path::Path) -> PathBuf {
        let path = dir.join(if cfg!(windows) {
            "fake-python.bat"
        } else {
            "fake-python.sh"
        });
        let script = if cfg!(windows) {
            "@echo off\r\necho mower-fake %*\r\nping -n 61 127.0.0.1 > nul\r\n".to_string()
        } else {
            "#!/bin/sh\necho \"mower-fake $@\"\nsleep 60\n".to_string()
        };
        std::fs::write(&path, script).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        path
    }

    fn setup_bundle(dir: &std::path::Path, checkout: &std::path::Path) {
        let bundle = dir.join("accounts/main/mower");
        std::fs::create_dir_all(&bundle).unwrap();
        std::fs::write(
            bundle.join("conf.yml"),
            "account: ops@example.com\nwebview:\n  port: 59000\n  token: old\nadb: 1.2.3.4:1\n",
        )
        .unwrap();
        std::fs::create_dir_all(checkout).unwrap();
    }

    #[tokio::test]
    async fn start_patches_conf_spawns_and_archives_log() {
        let tmp = tempfile::tempdir().unwrap();
        let checkout = tmp.path().join("checkout");
        setup_bundle(tmp.path(), &checkout);

        let ex = MowerProcessExecutor::new(fake_python(tmp.path()));
        let handle = ex.start(&ctx(tmp.path())).await.unwrap();
        assert!(handle.locator.starts_with("pid="));

        // conf 白名单改写已生效（其余键保留）
        let conf =
            std::fs::read_to_string(tmp.path().join("accounts/main/mower/conf.yml")).unwrap();
        assert!(conf.contains("adb: 127.0.0.1:2771"), "{conf}");
        assert!(conf.contains("port: 58110"));
        assert!(conf.contains("account: ops@example.com"), "用户键不得丢失");

        // 数据目录：config 符号链接 + tmp
        let data = tmp.path().join("accounts/main/mower-data");
        assert!(data.join("config").symlink_metadata().is_ok());
        assert!(data.join("tmp").is_dir());
        assert!(data.join("run_server.py").is_file());

        // 假 python 输出进入会话日志
        tokio::time::sleep(Duration::from_millis(200)).await;
        let log = std::fs::read_to_string(tmp.path().join("logs/sessions/7.log")).unwrap();
        assert!(log.contains("mower-fake"), "{log}");

        ex.drain(&handle, Duration::from_secs(2)).await.unwrap();
        assert_eq!(ex.health(&handle).await, ExecutorHealth::Dead);
    }

    #[tokio::test]
    async fn start_requires_port_and_bundle() {
        let tmp = tempfile::tempdir().unwrap();
        let checkout = tmp.path().join("checkout");
        std::fs::create_dir_all(&checkout).unwrap();
        // 无 bundle
        let mut c = ctx(tmp.path());
        let ex = MowerProcessExecutor::new(fake_python(tmp.path()));
        assert!(ex.start(&c).await.is_err());
        // 有 bundle 但无端口
        setup_bundle(tmp.path(), &checkout);
        c.mower_port = None;
        assert!(ex.start(&c).await.is_err());
    }
}
