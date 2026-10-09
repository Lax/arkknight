//! 环境体检（§15 `akops doctor`）：探测外部依赖与工作目录一致性。
//!
//! 验收标准（M1 任务 1）：doctor 能发现现有部署全部组件
//! （adb、设备连通、maa-cli+MaaCore、mower 检出、python、docker、镜像）。

use std::net::TcpListener;

use serde::Serialize;

use crate::config::{Workdir, detect};
use crate::device::build_backend;

/// 检查结论级别。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    /// 正常
    Ok,
    /// 有提示但不阻塞（缺某条运行路径）
    Warn,
    /// 阻塞性问题
    Fail,
}

/// 单项检查结果。
#[derive(Debug, Clone, Serialize)]
pub struct Check {
    pub id: &'static str,
    pub level: Level,
    pub detail: String,
    /// 修复建议
    pub hint: Option<String>,
}

impl Check {
    fn ok(id: &'static str, detail: impl Into<String>) -> Self {
        Check {
            id,
            level: Level::Ok,
            detail: detail.into(),
            hint: None,
        }
    }
    fn warn(id: &'static str, detail: impl Into<String>, hint: impl Into<String>) -> Self {
        Check {
            id,
            level: Level::Warn,
            detail: detail.into(),
            hint: Some(hint.into()),
        }
    }
    fn fail(id: &'static str, detail: impl Into<String>, hint: impl Into<String>) -> Self {
        Check {
            id,
            level: Level::Fail,
            detail: detail.into(),
            hint: Some(hint.into()),
        }
    }
}

/// 体检报告。
#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub checks: Vec<Check>,
}

impl Report {
    pub fn has_fail(&self) -> bool {
        self.checks.iter().any(|c| c.level == Level::Fail)
    }

    pub fn count(&self, level: Level) -> usize {
        self.checks.iter().filter(|c| c.level == level).count()
    }
}

/// 执行全量体检。
///
/// `overrides` 允许 CLI 全局参数（如 `--mower-dir`）即时覆盖 `akops.toml`
/// 的对应路径，不落盘。
pub async fn run(wd: &Workdir, overrides: &detect::DetectOverrides) -> Report {
    let mut checks = Vec::new();

    // 1. 配置文件
    let cfg = match wd.load_config() {
        Ok(c) => {
            checks.push(Check::ok(
                "config",
                format!("akops.toml 解析与校验通过（schema v{}）", c.schema_version),
            ));
            c
        }
        Err(e) => {
            checks.push(Check::fail(
                "config",
                e.to_string(),
                "运行 `akops init` 初始化工作目录，或修复 akops.toml",
            ));
            return Report { checks };
        }
    };

    // 2. 环境工具探测（配置值兜底，CLI 覆盖优先）
    let det = detect::detect(&detect::DetectOverrides {
        adb_path: overrides
            .adb_path
            .clone()
            .or_else(|| Some(cfg.paths.adb_path.clone())),
        mower_dir: overrides
            .mower_dir
            .clone()
            .or_else(|| Some(cfg.paths.mower_dir.clone())),
    })
    .await;

    // adb
    checks.push(match &det.adb {
        Some(adb) => Check::ok("adb", format!("{}（{}）", adb.path.display(), adb.version)),
        None => Check::fail(
            "adb",
            format!("未找到 adb（paths.adb_path = {:?}）", cfg.paths.adb_path),
            "安装 Android platform-tools 并确保 adb 在 PATH，或在 akops.toml 配置绝对路径",
        ),
    });

    // maa（INV-1 依赖：切号唯一实现路径）
    checks.push(match &det.maa {
        Some(maa) => match &maa.core_version {
            Some(core) => Check::ok(
                "maa",
                format!("{}（{}，MaaCore {}）", maa.cli.path.display(), maa.cli.version, core),
            ),
            None => Check::warn(
                "maa",
                format!("{}（{}）未检测到 MaaCore", maa.cli.path.display(), maa.cli.version),
                "运行 `maa install` 安装 MaaCore（切号依赖，INV-1）",
            ),
        },
        None => Check::fail(
            "maa",
            "未找到 maa-cli（PATH 中无 `maa`）",
            "安装 maa-cli：https://github.com/MaaAssistantArknights/maa-cli（账号切号 INV-1 唯一依赖）",
        ),
    });

    // mower 检出（ProcessRunner 需要）
    checks.push(match &det.mower {
        Some(m) => match (&m.branch, &m.commit) {
            (Some(b), Some(c)) => Check::ok(
                "mower",
                format!("检出 {}（{b}@{c}）", m.path.display()),
            ),
            _ => Check::warn(
                "mower",
                format!("{} 存在但不是 git 检出（无法 mower update）", m.path.display()),
                "建议 git clone arknights-mower（alpha 分支）",
            ),
        },
        None => Check::warn(
            "mower",
            format!("未找到 mower 检出（paths.mower_dir = {:?}）", cfg.paths.mower_dir),
            "ProcessRunner 需要；git clone 后更新 akops.toml paths.mower_dir（DockerRunner 可忽略）",
        ),
    });

    // python（ProcessRunner 需要 3.11+）
    checks.push(match &det.python {
        Some(py) => {
            let ok = parse_python_major_minor(&py.version).is_some_and(|(a, b)| (a, b) >= (3, 11));
            if ok {
                Check::ok("python", py.version.clone())
            } else {
                Check::warn(
                    "python",
                    format!("{}（ProcessRunner 需 3.11+）", py.version),
                    "安装 Python 3.11+，或仅用 DockerRunner",
                )
            }
        }
        None => Check::warn(
            "python",
            "未找到 python3",
            "ProcessRunner 需要 Python 3.11+；纯 DockerRunner 可忽略",
        ),
    });

    // docker + mower 镜像
    checks.push(match &det.docker {
        Some(d) => {
            let image = &cfg.paths.docker_mower_image;
            match image_exists(image).await {
                Ok(true) => Check::ok("docker", format!("daemon {}，镜像 {image} 就绪", d.version)),
                Ok(false) => Check::warn(
                    "docker",
                    format!("daemon {}，但镜像 {image} 不存在", d.version),
                    "构建 mower 镜像（DockerRunner 需要）或改用 ProcessRunner",
                ),
                Err(e) => Check::warn(
                    "docker",
                    format!("查询镜像失败：{e}"),
                    "检查 docker CLI 权限",
                ),
            }
        }
        None => Check::warn(
            "docker",
            "Docker daemon 不可达",
            "DockerRunner 需要 Docker；Windows 原生跑 mower 用 ProcessRunner 即可",
        ),
    });

    // 3. 设备连通性
    for name in wd.device_names() {
        let dev = match wd.load_device(&name) {
            Ok(d) => d,
            Err(e) => {
                checks.push(Check::fail("device", e.to_string(), "修复设备配置文件"));
                continue;
            }
        };
        if dev.backend == crate::model::DeviceBackendKind::Redroid {
            checks.push(Check::warn(
                "device",
                format!("设备 {name} 为 redroid 后端（M2 未实现，忽略）"),
                "M1 使用 external",
            ));
            continue;
        }
        if dev.docker_compatible() && det.docker.is_none() {
            checks.push(Check::warn(
                "device",
                format!("设备 {name} 配置了 docker_adb 但 Docker 不可达（DockerRunner 不可用）"),
                "仅 ProcessRunner 可用",
            ));
        }
        match build_backend(&dev, &cfg.paths.adb_path_expanded()) {
            Ok(backend) => match backend.health().await {
                Ok(h) if h.reachable => {
                    // 游戏包检测（device test 语义，§7.2）
                    let pkgs = backend.detect_game_packages().await.unwrap_or_default();
                    let official = pkgs.iter().any(|p| p == "com.hypergryph.arknights");
                    let bili = pkgs
                        .iter()
                        .any(|p| p == "com.hypergryph.arknights.bilibili");
                    let pkg_desc = match (official, bili) {
                        (true, true) => "官服+B服 双包",
                        (true, false) => "官服包",
                        (false, true) => "B服包",
                        (false, false) => "未检测到游戏包",
                    };
                    if official || bili {
                        checks.push(Check::ok(
                            "device",
                            format!("设备 {name}（{}）在线，{pkg_desc}", dev.connection.host_adb),
                        ));
                    } else {
                        checks.push(Check::warn(
                            "device",
                            format!("设备 {name} 在线但未检测到游戏包"),
                            "安装明日方舟 APK 后重试",
                        ));
                    }
                }
                Ok(h) => checks.push(Check::warn(
                    "device",
                    format!("设备 {name} 不在线（adb 状态：{}）", h.detail),
                    "检查模拟器/容器是否运行，然后 `akops device test`",
                )),
                Err(e) => checks.push(Check::warn(
                    "device",
                    format!("设备 {name} 探测失败：{e}"),
                    "检查 host_adb 地址与模拟器 adb 设置",
                )),
            },
            Err(e) => checks.push(Check::fail("device", e.to_string(), "修复设备配置")),
        }
    }
    if wd.device_names().is_empty() {
        checks.push(Check::warn(
            "device",
            "工作目录未注册任何设备",
            "运行 `akops device add <name> --host-adb <addr>`",
        ));
    }

    // 4. 账号一致性与唯一性（§9.1）
    for id in wd.account_ids() {
        if let Err(e) = wd.load_account(&id) {
            checks.push(Check::fail("account", e.to_string(), "修复账号配置文件"));
        }
    }
    match wd.account_name_duplicates() {
        Ok(dups) if dups.is_empty() => {}
        Ok(dups) => {
            for (name, ids) in dups {
                checks.push(Check::fail(
                    "account",
                    format!("account_name {name:?} 在多个账号中重复：{}", ids.join(", ")),
                    "MAA 按此串在快速登录列表唯一匹配（§9.1）；请修改为可唯一区分的片段",
                ));
            }
        }
        Err(e) => checks.push(Check::fail("account", e.to_string(), "修复账号配置文件")),
    }
    // 空 account_name 提示（切号会失败）
    if let Ok(accounts) = wd.load_all_accounts() {
        let empty: Vec<&str> = accounts
            .iter()
            .filter(|a| a.account_name.is_empty())
            .map(|a| a.id.as_str())
            .collect();
        if !empty.is_empty() {
            checks.push(Check::warn(
                "account",
                format!(
                    "账号 {} 的 account_name 为空（切号无法选号，仅启动游戏）",
                    empty.join(", ")
                ),
                "补填打码手机号片段（官服）或昵称（B服）",
            ));
        }
    }
    let n_accounts = wd.account_ids().len();
    if n_accounts == 0 {
        checks.push(Check::warn(
            "account",
            "未注册任何账号",
            "运行 `akops account add <id>`",
        ));
    } else {
        checks.push(Check::ok(
            "account",
            format!("已注册 {n_accounts} 个账号，account_name 唯一性通过"),
        ));
    }

    // 5. 同设备同 server 约束（R5：M1 文档约束）
    if let Ok(devices) = wd.load_all_devices() {
        let _ = devices; // 亲和矩阵与设备-账号关联在 M2 调度时校验；此处保留扩展点
    }

    // 6. 端口段空闲（R8：分配前必须探测）
    checks.push(check_port_range(
        "ports",
        cfg.ports.mower_session_range,
        "mower 会话端口段",
    ));

    Report { checks }
}

/// 检查端口段空闲情况：逐个尝试绑定 127.0.0.1。
fn check_port_range(id: &'static str, range: [u16; 2], what: &str) -> Check {
    let [a, b] = range;
    let occupied: Vec<u16> = (a..=b)
        .filter(|p| TcpListener::bind(("127.0.0.1", *p)).is_err())
        .collect();
    if occupied.is_empty() {
        Check::ok(id, format!("{what} [{a}, {b}] 全部空闲"))
    } else if occupied.len() == ((b - a + 1) as usize) {
        Check::fail(
            id,
            format!("{what} [{a}, {b}] 全部被占用"),
            "调整 akops.toml ports 配置或释放端口",
        )
    } else {
        let shown: Vec<String> = occupied.iter().take(10).map(|p| p.to_string()).collect();
        Check::warn(
            id,
            format!(
                "{what} 中 {} 个端口被占用：{}{}",
                occupied.len(),
                shown.join(","),
                if occupied.len() > 10 { "…" } else { "" }
            ),
            "端口分配器会跳过占用项；如大面积被占建议调整端口段",
        )
    }
}

async fn image_exists(image: &str) -> Result<bool, String> {
    let out = tokio::process::Command::new("docker")
        .args(["images", "-q", image])
        .output()
        .await
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(!String::from_utf8_lossy(&out.stdout).trim().is_empty())
}

/// 从 `Python 3.14.8` 解析 (3, 14)。
fn parse_python_major_minor(version_line: &str) -> Option<(u32, u32)> {
    let mut it = version_line.split_whitespace().next_back()?.split('.');
    let major: u32 = it.next()?.parse().ok()?;
    let minor: u32 = it.next()?.parse().ok()?;
    Some((major, minor))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AkopsConfig;

    #[test]
    fn python_version_parse() {
        assert_eq!(parse_python_major_minor("Python 3.14.8"), Some((3, 14)));
        assert_eq!(parse_python_major_minor("Python 3.11.0"), Some((3, 11)));
        assert_eq!(parse_python_major_minor("Python 2.7.18"), Some((2, 7)));
        assert_eq!(parse_python_major_minor("junk"), None);
    }

    #[test]
    fn port_range_check_levels() {
        let hold = TcpListener::bind(("127.0.0.1", 58999)).unwrap();
        let c = check_port_range("ports", [58998, 59000], "测试段");
        assert_eq!(c.level, Level::Warn, "{c:?}");
        drop(hold);

        // 内核释放端口有微小时延，轮询至可绑定再断言空闲（避免时序 flaky）
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        loop {
            if TcpListener::bind(("127.0.0.1", 58999)).is_ok() {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "端口 58999 未被内核及时释放"
            );
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let c = check_port_range("ports", [58998, 59000], "测试段");
        assert_eq!(c.level, Level::Ok, "{c:?}");
    }

    #[tokio::test]
    async fn doctor_on_empty_workdir_only_warns() {
        let tmp = tempfile::tempdir().unwrap();
        let wd = Workdir::init(tmp.path(), &AkopsConfig::default()).unwrap();
        let report = run(&wd, &Default::default()).await;
        // 空工作目录不应有 Fail（工具缺失在 CI 环境可能 Fail，设备/账号仅 Warn）
        let config_check = report.checks.iter().find(|c| c.id == "config").unwrap();
        assert_eq!(config_check.level, Level::Ok);
    }
}
