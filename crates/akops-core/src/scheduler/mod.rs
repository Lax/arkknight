//! 调度器（§10）：策略引擎 → 会话队列 → 设备租约 → 生命周期管理。
//!
//! **M1 任务 7 交付**。当前仅导出领域词汇；设计要点备忘：
//! - 三期演进：M1 单设备串行+静态时间窗；M2 多设备池+水位扩容；M3 事件驱动交叉
//! - 状态机见 [`crate::model::SessionState`]；任何迁移写 `session_events`
//! - 双超时：`slice_deadline`（正常轮转）与 `max_runtime_deadline`（硬上限）
//! - 看门狗：执行器 health 连续 N 次失败 → Draining → 按退避重排
//! - 退避：`{initial:5m, factor:2, max:60m}` 指数退避（失败不得空转重试）
//! - 设备租约：SQLite 事务获取；daemon 崩溃恢复按心跳超时回收孤儿租约
//! - 单设备排队：`(priority desc, 就绪等待时长 desc)`，同优先级 FIFO
//! - 就绪判定（§10.3）：enabled ∧ 处于时间窗 ∧ 无活跃会话 ∧ 未在退避；
//!   时间窗按游戏日界（官服 UTC-4 04:00）计算，不用本地零点

pub use crate::model::{ExecutorKind, RunnerKind, SessionOutcome, SessionState};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_state_lifecycle_table() {
        // 状态机迁移表驱动测试的占位锚点（M1 任务 7 扩充为完整迁移表）
        let cases: &[(SessionState, bool, bool)] = &[
            (SessionState::Created, false, true),
            (SessionState::Queued, false, true),
            (SessionState::Switching, false, true),
            (SessionState::Running, false, true),
            (SessionState::Draining, false, true),
            (SessionState::Finished, true, false),
            (SessionState::TimedOut, true, false),
            (SessionState::SwitchFailed, true, false),
            (SessionState::Failed, true, false),
            (SessionState::Cancelled, true, false),
        ];
        for (state, terminal, lease) in cases {
            assert_eq!(state.is_terminal(), *terminal, "{state:?}");
            assert_eq!(state.holds_lease(), *lease, "{state:?}");
        }
    }
}
