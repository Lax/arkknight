//! 域模型：Account / Device / Session / 调度词汇（设计文档 §6）。
//!
//! 这些类型同时是配置文件的 serde 形态（事实源，INV-4）：
//! - `accounts/<id>/account.toml` → [`account::Account`]
//! - `devices/<name>.toml` → [`device::Device`]
//! - 会话运行态在 SQLite（[`crate::store`]），此处只定义领域词汇。

pub mod account;
pub mod device;
pub mod session;

pub use account::{Account, AccountSchedule, ScheduledExecutor, Server, TimeWindow};
pub use device::{Device, DeviceBackendKind, DeviceConnection};
pub use session::{ExecutorKind, RunnerKind, SessionOutcome, SessionState};
