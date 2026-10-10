//! 配置体系：`arkknight.toml` schema（设计文档 §11.2）与工作目录布局（§11.1）。
//!
//! 工作目录文件树是账号/设备/策略的**单一事实源**（INV-4）：
//! SQLite 只存运行态与统计；物化产物（MAA TOML、mower bundle）可随时重建。

pub mod detect;
pub mod duration;

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::SCHEMA_VERSION;
use crate::error::{CoreError, Result};
use crate::model::account::Account;
use crate::model::device::Device;
use crate::model::session::{ExecutorKind, RunnerKind};

pub use duration::HumanDuration;

/// 工作目录主配置文件名。
pub const CONFIG_FILE: &str = "arkknight.toml";

/// 路径中的 `~` 展开（跨平台，§16）；其余原样返回。
pub fn expand_path(p: &str) -> PathBuf {
    if let Some(rest) = p.strip_prefix('~') {
        // 两个条件语义分层：`~` 前缀形态 vs 家目录可用，保持分立可读
        #[allow(clippy::collapsible_if)]
        if (rest.is_empty() || rest.starts_with('/') || rest.starts_with('\\'))
            && let Some(home) = dirs::home_dir()
        {
            return home.join(rest.trim_start_matches(['/', '\\']));
        }
    }
    PathBuf::from(p)
}

// ---------- arkknight.toml schema（§11.2） ----------

/// 主配置（`arkknight.toml`）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AkopsConfig {
    pub schema_version: u32,
    pub paths: PathsConfig,
    pub server: ServerConfig,
    pub scheduler: SchedulerConfig,
    pub ports: PortsConfig,
    pub device_defaults: DeviceDefaults,
    /// 本地定时事项（时间轴标注用，如「每晚 18:00 网络闪断五分钟」）
    pub local_events: Vec<LocalEvent>,
}

impl Default for AkopsConfig {
    fn default() -> Self {
        AkopsConfig {
            schema_version: SCHEMA_VERSION,
            paths: PathsConfig::default(),
            server: ServerConfig::default(),
            scheduler: SchedulerConfig::default(),
            ports: PortsConfig::default(),
            device_defaults: DeviceDefaults::default(),
            local_events: Vec::new(),
        }
    }
}

/// 工具与检出路径。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PathsConfig {
    /// maa-cli 安装根（`maa install` 管理的 MaaCore 所在）
    pub maa_dir: String,
    /// mower 检出（ProcessRunner 用）
    pub mower_dir: String,
    /// adb 可执行文件（默认 PATH 中的 `adb`）
    pub adb_path: String,
    /// DockerRunner 使用的 mower 镜像
    pub docker_mower_image: String,
    /// Docker 端点（空 = 本机默认：Linux unix socket / Windows 命名管道；
    /// 支持 `unix:///path`、`tcp://host:port`。容器内部署指向 socket-proxy）
    pub docker_host: String,
}

impl Default for PathsConfig {
    fn default() -> Self {
        PathsConfig {
            maa_dir: "~/.local/share/arkknight/maa".into(),
            mower_dir: "~/src/arknights-mower".into(),
            adb_path: "adb".into(),
            docker_mower_image: "arkknight-mower:latest".into(),
            docker_host: String::new(),
        }
    }
}

impl PathsConfig {
    pub fn maa_dir_expanded(&self) -> PathBuf {
        expand_path(&self.maa_dir)
    }
    pub fn mower_dir_expanded(&self) -> PathBuf {
        expand_path(&self.mower_dir)
    }
    pub fn adb_path_expanded(&self) -> PathBuf {
        expand_path(&self.adb_path)
    }
    /// Docker 端点：空串归一为 None（bollard 走本机默认）。
    pub fn docker_host_option(&self) -> Option<&str> {
        let h = self.docker_host.trim();
        (!h.is_empty()).then_some(h)
    }
}

/// Web 服务配置。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    /// 监听地址；非 loopback 且 token 为空时拒绝启动（§17）
    pub bind: String,
    pub port: u16,
    /// 非空时启用 Bearer / `?token=` 鉴权
    pub token: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        ServerConfig {
            bind: "127.0.0.1".into(),
            port: 7100,
            token: String::new(),
        }
    }
}

/// 退避参数（§10.2：失败任务必须指数退避，不得短周期空转重试）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BackoffConfig {
    pub initial: HumanDuration,
    pub max: HumanDuration,
    pub factor: f64,
}

impl Default for BackoffConfig {
    fn default() -> Self {
        BackoffConfig {
            initial: HumanDuration::parse("5m").unwrap(),
            max: HumanDuration::parse("60m").unwrap(),
            factor: 2.0,
        }
    }
}

/// 调度器全局策略。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SchedulerConfig {
    /// 展示时区（IANA 名；调度计算用游戏日界）
    pub timezone: String,
    /// 游戏日界（官服 UTC-4 的 04:00 刷新，本地表示）
    pub game_day_boundary: String,
    /// 默认时间片
    pub default_slice: HumanDuration,
    /// 单会话硬上限
    pub max_session_runtime: HumanDuration,
    /// 切号失败最大重试次数
    pub max_switch_retries: u32,
    /// 每账号每日至少一个完整时间片
    pub daily_guarantee: bool,
    /// 会话优雅停止等待（超时强杀，附录 D）
    pub drain_grace: HumanDuration,
    /// 执行器健康探测间隔
    pub watchdog_interval: HumanDuration,
    /// 连续失败次数阈值 → Draining（§10.2 看门狗）
    pub watchdog_threshold: u32,
    pub backoff: BackoffConfig,
    /// M3 跨账号交叉调度开关
    pub cross_account: bool,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        SchedulerConfig {
            timezone: "Asia/Shanghai".into(),
            game_day_boundary: "04:00".into(),
            default_slice: HumanDuration::parse("2h").unwrap(),
            max_session_runtime: HumanDuration::parse("6h").unwrap(),
            max_switch_retries: 2,
            daily_guarantee: true,
            drain_grace: HumanDuration::parse("2m").unwrap(),
            watchdog_interval: HumanDuration::parse("30s").unwrap(),
            watchdog_threshold: 3,
            backoff: BackoffConfig::default(),
            cross_account: false,
        }
    }
}

/// 端口段配置。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PortsConfig {
    /// mower 会话 Web UI 端口段 [起, 止]（含）
    pub mower_session_range: [u16; 2],
    /// redroid 宿主端口段（M2）
    pub redroid_host_range: [u16; 2],
}

impl Default for PortsConfig {
    fn default() -> Self {
        PortsConfig {
            mower_session_range: [58100, 58199],
            redroid_host_range: [28000, 28099],
        }
    }
}

/// 本地定时事项（仅时间轴标注与选窗参考，调度器不强制避让；
/// 例：每晚网络闪断、路由器定时重启）。start/end 本地 `HH:MM`，须 start < end。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalEvent {
    pub name: String,
    pub start: String,
    pub end: String,
}

impl LocalEvent {
    /// 与时间窗同规则：`end < start` 表示跨过自然午夜（如 23:50-00:10）。
    pub fn validate(&self) -> Result<()> {
        if self.name.trim().is_empty() {
            return Err(CoreError::Config("本地事项 name 不能为空".into()));
        }
        crate::model::account::validate_hhmm(&self.start, "本地事项 start")?;
        crate::model::account::validate_hhmm(&self.end, "本地事项 end")?;
        if self.start == self.end {
            return Err(CoreError::Config(format!(
                "本地事项 {} {}-{} 不合法：start 与 end 相同",
                self.name, self.start, self.end
            )));
        }
        Ok(())
    }
}

/// 扩容准入水位（M2）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WatermarkConfig {
    pub free_mem_gb: u32,
    pub cpu_idle_pct: u32,
}

impl Default for WatermarkConfig {
    fn default() -> Self {
        WatermarkConfig {
            free_mem_gb: 14,
            cpu_idle_pct: 20,
        }
    }
}

/// redroid 池模板（M2；先随 schema 解析）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RedroidDefaults {
    pub image: String,
    pub mem_limit_gb: u32,
    pub cpu_limit: u32,
    pub gpu: bool,
    pub watermark: WatermarkConfig,
}

impl Default for RedroidDefaults {
    fn default() -> Self {
        RedroidDefaults {
            image: String::new(),
            mem_limit_gb: 12,
            cpu_limit: 8,
            gpu: true,
            watermark: WatermarkConfig::default(),
        }
    }
}

/// 设备默认值集合。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DeviceDefaults {
    pub redroid: RedroidDefaults,
}

impl AkopsConfig {
    /// 语义校验（字段格式与范围；跨实体校验在 doctor）。
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(CoreError::Config(format!(
                "arkknight.toml schema_version={} 与当前支持 {} 不一致：{}",
                self.schema_version,
                SCHEMA_VERSION,
                if self.schema_version > SCHEMA_VERSION {
                    "请升级 arkknight"
                } else {
                    "配置来自旧版本，请运行 arkknight migrate"
                }
            )));
        }
        crate::model::account::validate_hhmm(
            &self.scheduler.game_day_boundary,
            "game_day_boundary",
        )?;
        if self.scheduler.timezone.trim().is_empty() {
            return Err(CoreError::Config("scheduler.timezone 不能为空".into()));
        }
        for (what, [a, b]) in [
            ("mower_session_range", self.ports.mower_session_range),
            ("redroid_host_range", self.ports.redroid_host_range),
        ] {
            if a == 0 || b < a {
                return Err(CoreError::Config(format!(
                    "ports.{what} 端口段 [{a}, {b}] 不合法"
                )));
            }
        }
        for ev in &self.local_events {
            ev.validate()?;
        }
        if !matches!(self.server.bind.as_str(), "127.0.0.1" | "::1" | "localhost")
            && self.server.token.is_empty()
        {
            // §17：暴露局域网必须启用 token；server 命令启动时硬校验，此处提前告警性质
            return Err(CoreError::Config(format!(
                "server.bind={} 非 loopback 时必须设置 token（§17 安全约束）",
                self.server.bind
            )));
        }
        Ok(())
    }
}

// ---------- 工作目录布局（§11.1） ----------

/// arkknight 工作目录。
#[derive(Debug, Clone)]
pub struct Workdir {
    pub root: PathBuf,
}

/// 需要创建的子目录（§11.1）。
const SUBDIRS: &[&str] = &[
    "devices",
    "accounts",
    "state",
    "logs",
    "logs/sessions",
    "export",
    "templates",
];

impl Workdir {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Workdir { root: root.into() }
    }

    pub fn config_path(&self) -> PathBuf {
        self.root.join(CONFIG_FILE)
    }
    pub fn devices_dir(&self) -> PathBuf {
        self.root.join("devices")
    }
    pub fn accounts_dir(&self) -> PathBuf {
        self.root.join("accounts")
    }
    pub fn state_dir(&self) -> PathBuf {
        self.root.join("state")
    }
    pub fn logs_dir(&self) -> PathBuf {
        self.root.join("logs")
    }
    pub fn export_dir(&self) -> PathBuf {
        self.root.join("export")
    }
    pub fn templates_dir(&self) -> PathBuf {
        self.root.join("templates")
    }
    pub fn db_path(&self) -> PathBuf {
        self.state_dir().join("arkknight.db")
    }
    pub fn daemon_json(&self) -> PathBuf {
        self.state_dir().join("daemon.json")
    }
    pub fn lock_file(&self) -> PathBuf {
        self.state_dir().join(".workdir.lock")
    }
    pub fn account_dir(&self, key: &str) -> PathBuf {
        self.accounts_dir().join(key)
    }
    pub fn account_file(&self, key: &str) -> PathBuf {
        self.account_dir(key).join("account.toml")
    }
    /// mower 基建排班计划（bundle 内，会话容器/进程直读直写）。
    pub fn mower_plan_path(&self, key: &str) -> PathBuf {
        self.account_dir(key).join("mower").join("plan.json")
    }
    /// maa 自定义任务目录（窗口 executor=maa 时的 task 名即此处文件名）。
    pub fn maa_tasks_dir(&self, key: &str) -> PathBuf {
        self.account_dir(key).join("maa").join("tasks")
    }
    /// maa 任务名 → 文件路径（防路径穿越：仅字母/数字/`-`/`_`）。
    pub fn maa_task_path(&self, key: &str, name: &str) -> Result<PathBuf> {
        let ok = !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        if !ok {
            return Err(CoreError::Config(format!(
                "任务名 {name:?} 不合法：仅字母/数字/`-`/`_`"
            )));
        }
        Ok(self.maa_tasks_dir(key).join(format!("{name}.toml")))
    }
    pub fn device_file(&self, name: &str) -> PathBuf {
        self.devices_dir().join(format!("{name}.toml"))
    }

    /// 从 `from` 向上逐级探测 `arkknight.toml`（§15 全局参数缺省规则）。
    pub fn discover(from: &Path) -> Result<Workdir> {
        let mut cur = Some(from);
        while let Some(dir) = cur {
            if dir.join(CONFIG_FILE).is_file() {
                return Ok(Workdir::new(dir));
            }
            cur = dir.parent();
        }
        Err(CoreError::NotFound(format!(
            "未在 {} 及其上级目录找到 {CONFIG_FILE}：请先运行 `arkknight init`",
            from.display()
        )))
    }

    /// 初始化工作目录：目录骨架 + 写入 arkknight.toml。已存在则拒绝（幂等安全）。
    pub fn init(dir: &Path, cfg: &AkopsConfig) -> Result<Workdir> {
        let wd = Workdir::new(dir);
        if wd.config_path().exists() {
            return Err(CoreError::Config(format!(
                "{} 已存在：工作目录已初始化，拒绝覆盖",
                wd.config_path().display()
            )));
        }
        for sub in SUBDIRS {
            let p = wd.root.join(sub);
            fs::create_dir_all(&p).map_err(|e| CoreError::io(&p, e))?;
        }
        wd.save_config(cfg)?;
        Ok(wd)
    }

    pub fn load_config(&self) -> Result<AkopsConfig> {
        let path = self.config_path();
        let raw = fs::read_to_string(&path).map_err(|e| CoreError::io(&path, e))?;
        let cfg: AkopsConfig = toml::from_str(&raw)
            .map_err(|e| CoreError::Config(format!("解析 {} 失败：{e}", path.display())))?;
        cfg.validate()?;
        Ok(cfg)
    }

    pub fn save_config(&self, cfg: &AkopsConfig) -> Result<()> {
        let path = self.config_path();
        let body = toml::to_string_pretty(cfg)
            .map_err(|e| CoreError::Config(format!("序列化 arkknight.toml 失败：{e}")))?;
        let header = format!(
            "# arkknight 工作目录主配置\n# schema 文档：docs/arkknight-design.md §11.2\n# schema_version = {}\n\n",
            cfg.schema_version
        );
        fs::write(&path, header + &body).map_err(|e| CoreError::io(&path, e))
    }

    /// 列出账号 key（按目录名排序；跳过无 account.toml 的目录）。
    pub fn account_keys(&self) -> Vec<String> {
        let mut ids: Vec<String> = fs::read_dir(self.accounts_dir())
            .map(|rd| {
                rd.filter_map(|e| e.ok())
                    .filter(|e| e.path().join("account.toml").is_file())
                    .filter_map(|e| e.file_name().into_string().ok())
                    .collect()
            })
            .unwrap_or_default();
        ids.sort();
        ids
    }

    /// 读取单个账号。
    pub fn load_account(&self, key: &str) -> Result<Account> {
        let path = self.account_file(key);
        let raw = fs::read_to_string(&path).map_err(|e| CoreError::io(&path, e))?;
        let acc: Account = toml::from_str(&raw)
            .map_err(|e| CoreError::Config(format!("解析 {} 失败：{e}", path.display())))?;
        if acc.key != key {
            return Err(CoreError::Config(format!(
                "{} 中 key={:?} 与目录名不一致",
                path.display(),
                acc.key
            )));
        }
        acc.validate()?;
        Ok(acc)
    }

    /// 读取全部账号（任一解析失败即失败）。
    pub fn load_all_accounts(&self) -> Result<Vec<Account>> {
        self.account_keys()
            .iter()
            .map(|key| self.load_account(key))
            .collect()
    }

    /// 列出设备名（按文件名排序）。
    pub fn device_names(&self) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(self.devices_dir())
            .map(|rd| {
                rd.filter_map(|e| e.ok())
                    .filter(|e| e.path().extension().is_some_and(|x| x == "toml"))
                    .filter_map(|e| {
                        e.path()
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .map(String::from)
                    })
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }

    pub fn load_device(&self, name: &str) -> Result<Device> {
        let path = self.device_file(name);
        let raw = fs::read_to_string(&path).map_err(|e| CoreError::io(&path, e))?;
        let dev: Device = toml::from_str(&raw)
            .map_err(|e| CoreError::Config(format!("解析 {} 失败：{e}", path.display())))?;
        if dev.name != name {
            return Err(CoreError::Config(format!(
                "{} 中 name={:?} 与文件名不一致",
                path.display(),
                dev.name
            )));
        }
        dev.validate()?;
        Ok(dev)
    }

    pub fn load_all_devices(&self) -> Result<Vec<Device>> {
        self.device_names()
            .iter()
            .map(|n| self.load_device(n))
            .collect()
    }

    /// 跨账号 `account_name` 唯一性检查（切号匹配串必须可唯一定位，§9.1）。
    /// 返回 (account_name, [重复的账号 key]) 列表（长度>1 即冲突）。
    pub fn account_name_duplicates(&self) -> Result<Vec<(String, Vec<String>)>> {
        let mut by_name: std::collections::BTreeMap<String, Vec<String>> = Default::default();
        for acc in self.load_all_accounts()? {
            if !acc.account_name.is_empty() {
                by_name.entry(acc.account_name).or_default().push(acc.key);
            }
        }
        Ok(by_name
            .into_iter()
            .filter(|(_, ids)| ids.len() > 1)
            .collect())
    }

    /// 写入账号（同时创建 maa/mower/mower-data 子目录，§11.1）。
    pub fn save_account(&self, acc: &Account) -> Result<()> {
        crate::model::account::validate_slug(&acc.key, "账号 key")?;
        let dir = self.account_dir(&acc.key);
        for sub in ["", "maa", "mower", "mower-data"] {
            let p = dir.join(sub);
            fs::create_dir_all(&p).map_err(|e| CoreError::io(&p, e))?;
        }
        let path = self.account_file(&acc.key);
        let body = toml::to_string_pretty(acc)
            .map_err(|e| CoreError::Config(format!("序列化账号失败：{e}")))?;
        fs::write(&path, body).map_err(|e| CoreError::io(&path, e))
    }

    /// 写入设备。
    pub fn save_device(&self, dev: &Device) -> Result<()> {
        crate::model::account::validate_slug(&dev.name, "设备名")?;
        let path = self.device_file(&dev.name);
        let body = toml::to_string_pretty(dev)
            .map_err(|e| CoreError::Config(format!("序列化设备失败：{e}")))?;
        fs::write(&path, body).map_err(|e| CoreError::io(&path, e))
    }

    /// 删除账号目录（危险：含物化产物与用户自定义任务，调用方须先确认）。
    pub fn remove_account(&self, key: &str) -> Result<()> {
        let dir = self.account_dir(key);
        if !dir.is_dir() {
            return Err(CoreError::NotFound(format!("账号 {key} 不存在")));
        }
        fs::remove_dir_all(&dir).map_err(|e| CoreError::io(&dir, e))
    }

    pub fn remove_device(&self, name: &str) -> Result<()> {
        let path = self.device_file(name);
        if !path.is_file() {
            return Err(CoreError::NotFound(format!("设备 {name} 不存在")));
        }
        fs::remove_file(&path).map_err(|e| CoreError::io(&path, e))
    }
}

/// Runner 形态 → 该用的 adb 地址（双地址选择规则，§8.3/§11.3）。
pub fn adb_address_for(dev: &Device, runner: RunnerKind) -> Result<String> {
    match runner {
        RunnerKind::Docker => dev.connection.docker_adb.clone().ok_or_else(|| {
            CoreError::Config(format!(
                "设备 {} 未配置 docker_adb，无法用 DockerRunner",
                dev.name
            ))
        }),
        RunnerKind::Process => Ok(dev.connection.host_adb.clone()),
    }
}

/// 执行器类型在 CLI/JSON 的展示名。
pub fn executor_name(k: ExecutorKind) -> &'static str {
    match k {
        ExecutorKind::Maa => "maa",
        ExecutorKind::Mower => "mower",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_workdir() -> (tempfile::TempDir, Workdir) {
        let tmp = tempfile::tempdir().unwrap();
        let wd = Workdir::init(tmp.path(), &AkopsConfig::default()).unwrap();
        (tmp, wd)
    }

    #[test]
    fn init_creates_skeleton_and_config_roundtrip() {
        let (_tmp, wd) = tmp_workdir();
        for sub in SUBDIRS {
            assert!(wd.root.join(sub).is_dir(), "缺目录 {sub}");
        }
        let cfg = wd.load_config().unwrap();
        assert_eq!(cfg, AkopsConfig::default());
        assert_eq!(cfg.scheduler.default_slice.0.as_secs(), 7200);

        // 已初始化的目录拒绝再次 init
        assert!(Workdir::init(wd.root.as_path(), &cfg).is_err());
    }

    #[test]
    fn discover_walks_up() {
        let (tmp, wd) = tmp_workdir();
        let nested = tmp.path().join("a/b/c");
        std::fs::create_dir_all(&nested).unwrap();
        let found = Workdir::discover(&nested).unwrap();
        assert_eq!(found.root, wd.root);
        let miss = Workdir::discover(tempfile::tempdir().unwrap().path());
        assert!(miss.is_err());
    }

    #[test]
    fn account_and_device_crud_with_uniqueness() {
        let (_tmp, wd) = tmp_workdir();
        let mk = |id: &str, name: &str| Account {
            key: id.into(),
            display_name: id.into(),
            server: crate::model::Server::Official,
            account_name: name.into(),
            uid: None,
            enabled: true,
            schedule: Default::default(),
            provisioned_on: vec![],
        };
        wd.save_account(&mk("a", "123****0001")).unwrap();
        wd.save_account(&mk("b", "123****0002")).unwrap();
        assert_eq!(wd.account_keys(), vec!["a", "b"]);
        assert_eq!(wd.account_name_duplicates().unwrap().len(), 0);

        wd.save_account(&mk("c", "123****0001")).unwrap();
        let dups = wd.account_name_duplicates().unwrap();
        assert_eq!(
            dups,
            vec![("123****0001".into(), vec!["a".into(), "c".into()])]
        );

        let dev = Device {
            name: "d1".into(),
            backend: crate::model::DeviceBackendKind::External,
            connection: crate::model::DeviceConnection {
                host_adb: "127.0.0.1:2771".into(),
                docker_adb: None,
                docker_network: None,
            },
            notes: String::new(),
        };
        wd.save_device(&dev).unwrap();
        assert_eq!(wd.load_all_devices().unwrap().len(), 1);
        assert!(wd.remove_device("d1").is_ok());
        assert!(wd.remove_device("d1").is_err());
    }

    #[test]
    fn local_event_validation() {
        let mut cfg = AkopsConfig {
            local_events: vec![LocalEvent {
                name: "网络闪断".into(),
                start: "18:00".into(),
                end: "18:05".into(),
            }],
            ..AkopsConfig::default()
        };
        cfg.validate().unwrap();
        // 非法 HH:MM
        cfg.local_events[0].start = "18:0".into();
        assert!(cfg.validate().is_err());
        // start == end（零长度）拒绝
        cfg.local_events[0].start = "18:00".into();
        cfg.local_events[0].end = "18:00".into();
        assert!(cfg.validate().is_err());
        // 跨自然天合法（end < start = 跨过午夜）
        cfg.local_events[0].start = "23:50".into();
        cfg.local_events[0].end = "00:10".into();
        cfg.validate().unwrap();
    }

    #[test]
    fn config_validation_rules() {
        let mut cfg = AkopsConfig::default();
        cfg.validate().unwrap();
        cfg.schema_version = SCHEMA_VERSION + 1;
        assert!(cfg.validate().is_err());
        cfg = AkopsConfig::default();
        cfg.scheduler.game_day_boundary = "4:0".into();
        assert!(cfg.validate().is_err());
        cfg = AkopsConfig::default();
        cfg.server.bind = "0.0.0.0".into();
        assert!(cfg.validate().is_err(), "非 loopback 空 token 必须拒绝");
        cfg.server.token = "secret".into();
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn adb_address_selection_by_runner() {
        let dev = Device {
            name: "d".into(),
            backend: crate::model::DeviceBackendKind::External,
            connection: crate::model::DeviceConnection {
                host_adb: "127.0.0.1:2771".into(),
                docker_adb: Some("arknights:5555".into()),
                docker_network: Some("arknights_default".into()),
            },
            notes: String::new(),
        };
        assert_eq!(
            adb_address_for(&dev, RunnerKind::Process).unwrap(),
            "127.0.0.1:2771"
        );
        assert_eq!(
            adb_address_for(&dev, RunnerKind::Docker).unwrap(),
            "arknights:5555"
        );

        let host_only = Device {
            connection: crate::model::DeviceConnection {
                host_adb: "127.0.0.1:16384".into(),
                docker_adb: None,
                docker_network: None,
            },
            ..dev
        };
        assert!(adb_address_for(&host_only, RunnerKind::Docker).is_err());
    }

    #[test]
    fn expand_tilde() {
        if dirs::home_dir().is_some() {
            assert!(expand_path("~/x").is_absolute());
            assert_eq!(expand_path("/a/b"), PathBuf::from("/a/b"));
        }
    }
}
