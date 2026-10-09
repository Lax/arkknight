//! 执行器层：Executor trait（§8.1）。
//!
//! INV-3：调度器只面向本 trait 编程，新增执行器不得修改调度器核心。
//! 具体实现按里程碑交付：
//! - [`maa`]（M1 任务 4）：MAA 任务会话；切号走 [`crate::switch`]（INV-1 唯一入口）
//! - mower（M1 任务 6）：基建会话，Runner 子层 Docker|Process（ADR-0001 D5）
//! - 预留：ScriptExecutor 等第三方

pub mod maa;
pub mod mower;

use std::time::Duration;

use serde::Serialize;

use crate::device::DeviceEndpoints;
use crate::model::{Account, ExecutorKind, RunnerKind};

pub use maa::MaaCliExecutor;
pub use mower::{MowerProcessExecutor, parse_locator, stop_via_http};

/// 执行器错误。
#[derive(Debug, thiserror::Error)]
pub enum ExecutorError {
    #[error("执行器启动失败：{0}")]
    Start(String),

    #[error("执行器停止失败：{0}")]
    Drain(String),

    #[error("{0}")]
    Other(String),
}

/// 会话上下文（执行器启动入参；调度器在持有设备租约并完成切号后构造）。
#[derive(Debug, Clone)]
pub struct SessionCtx {
    /// SQLite 会话 id
    pub session_id: u64,
    /// 目标账号
    pub account: Account,
    /// 设备双视角端点（执行器按自身 Runner 形态取用）
    pub endpoints: DeviceEndpoints,
    /// mower 会话运行形态（MAA 会话恒为 Process 语义，字段仍可填充）
    pub runner: RunnerKind,
    /// 已分配的 mower Web UI 端口（仅 mower 会话）
    pub mower_port: Option<u16>,
    /// 工作目录根（执行器在其下定位账号 bundle）
    pub workdir: std::path::PathBuf,
    /// mower 检出根（ProcessRunner cwd 与启动器路径来源；来自 paths.mower_dir）
    pub mower_checkout: std::path::PathBuf,
    /// MAA 任务名（仅 MAA 会话；None 时 MaaCliExecutor 拒绝启动）
    pub maa_task: Option<String>,
}

impl SessionCtx {
    /// mower webview 会话 token（深链拼接用）。
    ///
    /// M1 采用可预测值（仅面向 localhost/内网）；server 暴露公网时由任务 8
    /// 升级为随机 token 并经 /api 下发。
    pub fn webview_token(&self) -> String {
        format!("arkreunion-s{}", self.session_id)
    }
}

/// 执行器句柄：一次启动的唯一凭据（容器 id / 子进程 pid 等）。
#[derive(Debug, Clone, Serialize)]
pub struct ExecutorHandle {
    pub session_id: u64,
    pub kind: ExecutorKind,
    pub runner: RunnerKind,
    /// 形态相关的定位串（如 `pid=1234`、`container=abc123`）
    pub locator: String,
}

/// 执行器健康探测结果（喂看门狗，§10.2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ExecutorHealth {
    /// 存活且在工作
    Alive,
    /// 进程在但无心跳/失联
    Stalled,
    /// 已退出
    Dead,
    /// 无法判定（探测本身失败；按 Stalled 处理并计数）
    Unknown,
}

/// 执行器 trait（§8.1）。
#[async_trait::async_trait]
pub trait Executor: Send + Sync {
    fn kind(&self) -> ExecutorKind;

    /// 启动执行器会话（假定账号切换已完成、设备租约已持有）。
    async fn start(&self, ctx: &SessionCtx) -> Result<ExecutorHandle, ExecutorError>;

    /// 优雅停止（mower：`POST /stop` → 等待退出；MAA：等任务进程结束/kill）。
    async fn drain(&self, handle: &ExecutorHandle, grace: Duration) -> Result<(), ExecutorError>;

    /// 健康探测。
    async fn health(&self, handle: &ExecutorHandle) -> ExecutorHealth;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// FakeExecutor：CI 无设备跑调度全场景用的假件（§19），M1 任务 7 前先验证 trait 可实现性。
    struct FakeExecutor;

    #[async_trait::async_trait]
    impl Executor for FakeExecutor {
        fn kind(&self) -> ExecutorKind {
            ExecutorKind::Mower
        }
        async fn start(&self, ctx: &SessionCtx) -> Result<ExecutorHandle, ExecutorError> {
            Ok(ExecutorHandle {
                session_id: ctx.session_id,
                kind: self.kind(),
                runner: ctx.runner,
                locator: "fake".into(),
            })
        }
        async fn drain(&self, _h: &ExecutorHandle, _g: Duration) -> Result<(), ExecutorError> {
            Ok(())
        }
        async fn health(&self, _h: &ExecutorHandle) -> ExecutorHealth {
            ExecutorHealth::Alive
        }
    }

    #[tokio::test]
    async fn trait_object_usable() {
        let ex: Box<dyn Executor> = Box::new(FakeExecutor);
        let ctx = SessionCtx {
            session_id: 1,
            account: crate::model::Account {
                key: "a".into(),
                display_name: "a".into(),
                server: crate::model::Server::Official,
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
            mower_port: Some(58100),
            workdir: std::env::temp_dir(),
            mower_checkout: std::env::temp_dir(),
            maa_task: None,
        };
        let h = ex.start(&ctx).await.unwrap();
        assert_eq!(ex.health(&h).await, ExecutorHealth::Alive);
        ex.drain(&h, Duration::from_secs(1)).await.unwrap();
    }
}
