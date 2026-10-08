//! akops-core —— 明日方舟多账号编排器核心库。
//!
//! akops 编排 MAA（账号切换/任务）与 mower（基建排班）等执行器，
//! 在少量安卓设备上按时间窗自动轮转运行多个游戏账号。
//! 本库自身**不做任何游戏内图像识别与操作自动化**。
//!
//! 模块地图（对应设计文档 `docs/akops-design.md`）：
//! - [`model`]：域模型 Account / Device / Session（§6）
//! - [`config`]：akops.toml 解析、工作目录布局、环境探测（§11）
//! - [`materialize`]：MAA/mower 配置物化与白名单改写（§11.3）
//! - [`device`]：DeviceBackend trait + external 后端（§7）
//! - [`executor`]：Executor trait（§8，MAA/mower 具体实现按里程碑交付）
//! - [`scheduler`]：调度器（§10，M1 任务 7 交付）
//! - [`switch`]：账号切换唯一入口（§9，受 INV-1 约束，M1 任务 4 交付）
//! - [`store`]：SQLite 运行态持久化（§14，M1 任务 3/7 交付）
//! - [`doctor`]：环境体检（§15 doctor 命令）
//!
//! 硬性不变量（INV-1~4）见设计文档 §5 与 `docs/ai/AGENTS.md`，违反即拒改。

pub mod config;
pub mod device;
pub mod doctor;
pub mod error;
pub mod executor;
pub mod materialize;
pub mod model;
pub mod scheduler;
pub mod store;
pub mod switch;

/// 工作目录配置 schema 版本（`akops.toml` 的 `schema_version`）。
pub const SCHEMA_VERSION: u32 = 1;
