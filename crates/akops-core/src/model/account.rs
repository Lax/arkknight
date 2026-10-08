//! 账号模型（设计文档 §6.1）。

use crate::config::HumanDuration;
use crate::error::{CoreError, Result};
use serde::{Deserialize, Serialize};

/// 服务器类型（影响 MAA client_type、包名、资源差异）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Server {
    /// 官服
    Official,
    /// B 服
    Bilibili,
}

impl Server {
    /// MAA 任务参数 `client_type` 的取值。
    pub fn maa_client_type(self) -> &'static str {
        match self {
            Server::Official => "Official",
            Server::Bilibili => "Bilibili",
        }
    }

    /// 游戏安卓包名。
    pub fn game_package(self) -> &'static str {
        match self {
            Server::Official => "com.hypergryph.arknights",
            Server::Bilibili => "com.hypergryph.arknights.bilibili",
        }
    }
}

impl std::str::FromStr for Server {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "official" => Ok(Server::Official),
            "bilibili" => Ok(Server::Bilibili),
            other => Err(format!("server {other:?} 不合法：official|bilibili")),
        }
    }
}

impl std::fmt::Display for Server {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Server::Official => "official",
            Server::Bilibili => "bilibili",
        })
    }
}

/// 时间窗内运行的执行器类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScheduledExecutor {
    /// mower 基建排班会话（默认）
    Mower,
    /// MAA 任务会话（周计划刷图、肉鸽等，用户自定义 tasks）
    Maa,
}

/// 每日时间窗（游戏日界内，`HH:MM` 24 小时制）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeWindow {
    /// 开始时刻 `HH:MM`（本地时区，游戏日为 04:00 起）
    pub start: String,
    /// 结束时刻 `HH:MM`
    pub end: String,
    /// 该窗口运行的执行器
    pub executor: ScheduledExecutor,
}

/// 账号调度策略。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AccountSchedule {
    /// 每日时间窗；为空表示该账号不参与自动调度（仅手动会话）。
    pub windows: Vec<TimeWindow>,
    /// 队列优先级 0-100（默认 50；手动会话=100 插队）
    pub priority: u8,
    /// 时间片长度；None 用全局 `scheduler.default_slice`
    pub slice: Option<HumanDuration>,
}

impl Default for AccountSchedule {
    fn default() -> Self {
        AccountSchedule {
            windows: Vec::new(),
            priority: 50,
            slice: None,
        }
    }
}

fn default_true() -> bool {
    true
}

/// 账号（`accounts/<id>/account.toml`，设计文档 §6.1）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    /// 唯一标识（= 目录名，slug：小写字母/数字/`-`/`_`）
    pub id: String,
    /// 展示名
    pub display_name: String,
    /// 服务器类型
    pub server: Server,
    /// **MAA 切号匹配串**：官服=打码手机号片段（如 `123****8901`），B 服=昵称。
    /// 须在该设备已登录账号中唯一（最终以 MAA 运行结果为准）。
    pub account_name: String,
    /// 禁用后不参与调度
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// 调度策略
    #[serde(default)]
    pub schedule: AccountSchedule,
    /// 已人工登录过的设备名（亲和种子；正式记录在 SQLite `logins` 表）
    #[serde(default)]
    pub provisioned_on: Vec<String>,
}

impl Account {
    /// 校验账号配置（slug、时间窗格式、优先级范围）。
    pub fn validate(&self) -> Result<()> {
        validate_slug(&self.id, "账号 id")?;
        for w in &self.schedule.windows {
            validate_hhmm(&w.start, "时间窗 start")?;
            validate_hhmm(&w.end, "时间窗 end")?;
        }
        if self.schedule.priority > 100 {
            return Err(CoreError::Config(format!(
                "账号 {} 的 priority={} 超出 0-100",
                self.id, self.schedule.priority
            )));
        }
        Ok(())
    }
}

/// 校验 slug（小写字母/数字开头，含 `-`/`_`）。
pub fn validate_slug(name: &str, what: &str) -> Result<()> {
    let ok = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
        && name
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
    if ok {
        Ok(())
    } else {
        Err(CoreError::Config(format!(
            "{what} `{name}` 不合法：须为小写字母/数字开头，仅含小写字母、数字、`-`、`_`"
        )))
    }
}

/// 校验严格 `HH:MM`（两位小时 + 冒号 + 两位分钟，24 小时制）。
pub(crate) fn validate_hhmm(s: &str, what: &str) -> Result<()> {
    let bad = || CoreError::Config(format!("{what} `{s}` 不合法：须为严格 `HH:MM` 24 小时制"));
    let b = s.as_bytes();
    if b.len() != 5 || b[2] != b':' {
        return Err(bad());
    }
    let h: u32 = s[0..2].parse().map_err(|_| bad())?;
    let m: u32 = s[3..5].parse().map_err(|_| bad())?;
    if h <= 23 && m <= 59 {
        Ok(())
    } else {
        Err(bad())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_toml_roundtrip() {
        let toml_src = r#"
id = "main"
display_name = "主号"
server = "official"
account_name = "123****8901"
enabled = true
provisioned_on = ["redroid-main"]

[schedule]
priority = 60
slice = "90m"

[[schedule.windows]]
start = "08:00"
end = "12:00"
executor = "mower"

[[schedule.windows]]
start = "20:00"
end = "22:00"
executor = "maa"
"#;
        let acc: Account = toml::from_str(toml_src).unwrap();
        assert_eq!(acc.server, Server::Official);
        assert_eq!(acc.schedule.priority, 60);
        assert_eq!(acc.schedule.windows.len(), 2);
        assert_eq!(acc.schedule.windows[0].executor, ScheduledExecutor::Mower);
        assert_eq!(acc.schedule.windows[1].executor, ScheduledExecutor::Maa);
        assert_eq!(acc.provisioned_on, vec!["redroid-main"]);
        acc.validate().unwrap();

        let out = toml::to_string_pretty(&acc).unwrap();
        let acc2: Account = toml::from_str(&out).unwrap();
        assert_eq!(acc, acc2);
    }

    #[test]
    fn defaults_apply() {
        let acc: Account = toml::from_str(
            r#"
id = "alt"
display_name = "小号"
server = "bilibili"
account_name = "昵称"
"#,
        )
        .unwrap();
        assert!(acc.enabled);
        assert_eq!(acc.schedule.priority, 50);
        assert!(acc.schedule.windows.is_empty());
        assert_eq!(
            acc.server.game_package(),
            "com.hypergryph.arknights.bilibili"
        );
    }

    #[test]
    fn slug_and_hhmm_validation() {
        assert!(validate_slug("main", "x").is_ok());
        assert!(validate_slug("Main", "x").is_err());
        assert!(validate_slug("", "x").is_err());
        assert!(validate_slug("-x", "x").is_err());
        assert!(validate_hhmm("04:00", "x").is_ok());
        assert!(validate_hhmm("4:00", "x").is_err());
        assert!(validate_hhmm("24:00", "x").is_err());
    }
}
