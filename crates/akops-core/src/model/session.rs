//! 会话领域词汇（设计文档 §6.3、§10.2）。
//!
//! 会话运行态记录（SQLite `sessions` 表行）由 store 层（M1 任务 3/7）落地，
//! 此处先定义领域词汇与状态机，供模型与调度器（M1 任务 7）共享。

use serde::{Deserialize, Serialize};

/// 执行器类型（§8.1 `Executor::kind()`；serde 形态与配置文件一致）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExecutorKind {
    /// MAA 任务（含切号 startup）
    Maa,
    /// mower 基建排班
    Mower,
}

/// mower 会话运行形态（Runner 子层，ADR-0001 D5）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RunnerKind {
    /// bollard 容器（Linux / Docker Desktop）
    Docker,
    /// 本地进程（`python run_server.py`，任意平台）
    Process,
}

/// 会话状态机（§10.2）。
///
/// ```text
/// Created → Queued →(获得设备租约)→ Switching → Running → Draining → Finished/TimedOut
///                        │            │
///                        │            ├─ 切号重试耗尽 → SwitchFailed
///                        │            └─ 执行器崩溃不可恢复 → Failed
///                        └─ 取消 → Cancelled
/// 任何状态 → Cancelled（手动）
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionState {
    Created,
    Queued,
    Switching,
    Running,
    Draining,
    Finished,
    TimedOut,
    SwitchFailed,
    Failed,
    Cancelled,
}

impl SessionState {
    /// 是否终态（不再迁移）。
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            SessionState::Finished
                | SessionState::TimedOut
                | SessionState::SwitchFailed
                | SessionState::Failed
                | SessionState::Cancelled
        )
    }

    /// 是否占用设备租约。
    pub fn holds_lease(self) -> bool {
        matches!(
            self,
            SessionState::Created
                | SessionState::Queued
                | SessionState::Switching
                | SessionState::Running
                | SessionState::Draining
        )
    }
}

/// 会话结果（终态归因）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionOutcome {
    Completed,
    SliceExpired,
    MaxRuntime,
    Watchdog,
    SwitchFailure,
    ExecutorCrash,
    Cancelled,
}

impl SessionOutcome {
    /// serde/存储名（snake_case，与 `#[serde(rename_all)]` 一致）。
    pub fn name(self) -> &'static str {
        match self {
            SessionOutcome::Completed => "completed",
            SessionOutcome::SliceExpired => "slice_expired",
            SessionOutcome::MaxRuntime => "max_runtime",
            SessionOutcome::Watchdog => "watchdog",
            SessionOutcome::SwitchFailure => "switch_failure",
            SessionOutcome::ExecutorCrash => "executor_crash",
            SessionOutcome::Cancelled => "cancelled",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_serde_and_lifecycle() {
        assert_eq!(
            serde_json::to_value(SessionState::SwitchFailed).unwrap(),
            serde_json::json!("switch_failed")
        );
        assert!(SessionState::Finished.is_terminal());
        assert!(!SessionState::Draining.is_terminal());
        assert!(SessionState::Switching.holds_lease());
        assert!(!SessionState::Finished.holds_lease());
    }
}
