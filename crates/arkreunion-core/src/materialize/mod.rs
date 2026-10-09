//! 物化器（§11.3）：从统一配置生成执行器可直接消费的配置。
//!
//! - MAA：`accounts/<key>/maa/profiles/default.toml` + `tasks/startup.toml`
//! - mower：会话启动瞬间对 `conf.yml` 的**白名单改写**（仅 `adb`、
//!   `start_automatically`、`webview.port`、`webview.token` 四项，§11.4），
//!   其余键（含用户通过 mower UI 的编辑）原样保留
//!
//! 幂等性：物化是纯函数（除白名单字段），重复执行结果一致。

pub mod maa;
pub mod mower;

pub use maa::{
    render_profile, render_startup_task, render_uid_check_pipeline, render_uid_check_task,
};
pub use mower::{ensure_process_data_dir, patch_conf, render_process_launcher};

/// mower conf.yml 白名单改写参数（会话启动瞬间，§8.3）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MowerPatch {
    /// 按 Runner 形态选择的 adb 地址
    pub adb: String,
    /// 分配的 Web UI 端口（58100-58199 记账分配）
    pub port: u16,
    /// 会话 token（深链拼接用；None 则保留原值）
    pub token: Option<String>,
    pub start_automatically: bool,
}

/// MAA connection.config 的平台适配值（设计文档附录 B：物化器按 OS 生成）。
pub fn maa_connection_config() -> &'static str {
    if cfg!(windows) {
        "CompatWin32Shell"
    } else {
        "CompatPOSIXShell"
    }
}
