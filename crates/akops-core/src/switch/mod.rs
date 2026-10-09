//! 账号切换（§9）：**INV-1 唯一实现入口。**
//!
//! 唯一路径：[`run_switch`] → `maa run startup -p default --batch`
//! （StartUp 任务 + `account_name` 参数，MAA 官方能力）。
//!
//! **禁止**出现：登录界面截图识别、坐标点击、OCR、输入法模拟等任何自研
//! 登录自动化代码。MAA 不可用/切号失败 → 重试耗尽后 `SwitchFailed` + 告警，
//! 不降级为其他手段。
//!
//! 流程（§9.1）：持有设备租约 → force-stop 游戏 → 物化 MAA 配置（宿主视角
//! address）→ `maa run startup`（超时/重试/指数退避）→ 记 `switch_log` 与
//! 会话终态。亲和前提（§9.2）：仅对已 provision 的 (账号, 设备) 有效。

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::config::{AkopsConfig, Workdir};
use crate::error::{CoreError, Result};
use crate::materialize::{render_profile, render_startup_task};
use crate::model::{Device, RunnerKind};
use crate::store::{Store, StoreError, SwitchLogEntry};

/// 一次切号的执行上下文（maa/adb 路径可注入，便于测试）。
pub struct SwitchCtx {
    pub wd: Workdir,
    pub cfg: AkopsConfig,
    pub store: std::sync::Arc<Store>,
    pub account: crate::model::Account,
    pub device: Device,
    /// maa-cli 可执行文件（默认 `maa`）
    pub maa_bin: PathBuf,
    /// adb 可执行文件
    pub adb_bin: PathBuf,
    /// 单次尝试超时
    pub timeout: Duration,
    /// 覆盖退避初始间隔（测试用；None 用配置值）
    pub backoff_initial_override: Option<Duration>,
}

/// 切号结果。
#[derive(Debug, Clone)]
pub struct SwitchOutcome {
    pub ok: bool,
    pub duration_ms: i64,
    pub attempts: u32,
    pub error: Option<String>,
    pub maa_log_excerpt: Option<String>,
}

impl SwitchCtx {
    /// 从工作目录与配置构造默认上下文。
    pub fn new(
        wd: &Workdir,
        cfg: &AkopsConfig,
        store: std::sync::Arc<Store>,
        account: crate::model::Account,
        device: Device,
    ) -> Self {
        SwitchCtx {
            wd: wd.clone(),
            cfg: cfg.clone(),
            store,
            account,
            device,
            maa_bin: PathBuf::from("maa"),
            adb_bin: cfg.paths.adb_path_expanded(),
            timeout: Duration::from_secs(5 * 60),
            backoff_initial_override: None,
        }
    }
}

/// 切号机械计划（无 store/会话/租约参与；CLI [`run_switch`] 与调度器共用）。
pub struct SwitchPlan<'a> {
    pub wd: &'a Workdir,
    pub cfg: &'a AkopsConfig,
    pub account: &'a crate::model::Account,
    pub device: &'a Device,
    pub maa_bin: &'a Path,
    pub adb_bin: &'a Path,
    /// 单次尝试超时
    pub timeout: Duration,
    /// 覆盖退避初始间隔（测试用；None 用配置值）
    pub backoff_initial_override: Option<Duration>,
}

/// 执行切号机械步骤：force-stop → 物化 MAA 配置 → `maa run startup` 重试环。
/// 纯 mechanics——不触碰会话行/租约/switch_log，由调用方负责（§9.1）。
pub async fn execute_switch(plan: &SwitchPlan<'_>) -> SwitchOutcome {
    let started = std::time::Instant::now();
    let account = plan.account;
    let device = plan.device;

    // force-stop 游戏（按 server 选包名；游戏未运行时也返回成功）
    let package = account.server.game_package();
    let _ = adb_shell(
        plan.adb_bin,
        &device.connection.host_adb,
        &["am", "force-stop", package],
    )
    .await;

    // 物化 MAA 配置（切号从宿主发起 → Process 视角地址，§9.1）
    let maa_dir = plan.wd.account_dir(&account.id).join("maa");
    let endpoints = crate::device::DeviceEndpoints {
        host_adb: device.connection.host_adb.clone(),
        docker_adb: device.connection.docker_adb.clone(),
        docker_network: device.connection.docker_network.clone(),
    };
    if let Err(e) = materialize_maa(&maa_dir, account, &endpoints) {
        return SwitchOutcome {
            ok: false,
            duration_ms: started.elapsed().as_millis() as i64,
            attempts: 0,
            error: Some(e.to_string()),
            maa_log_excerpt: None,
        };
    }

    // 重试环（≤ max_switch_retries + 1 次，指数退避）
    let retries_max = plan.cfg.scheduler.max_switch_retries;
    let mut attempts: u32 = 0;
    let mut last_error: Option<String> = None;
    let mut excerpt: Option<String> = None;
    let mut ok = false;
    while attempts <= retries_max {
        attempts += 1;
        tracing::info!(attempt = attempts, "maa run startup 第 {attempts} 次尝试");
        match run_maa_startup(plan.maa_bin, plan.timeout, &maa_dir).await {
            Ok(out) => {
                ok = true;
                excerpt = Some(out);
                last_error = None;
                break;
            }
            Err(e) => {
                tracing::warn!(attempt = attempts, "切号尝试失败：{e}");
                last_error = Some(e);
                if attempts <= retries_max {
                    let backoff = backoff_delay(
                        &plan.cfg.scheduler.backoff,
                        plan.backoff_initial_override,
                        attempts,
                    );
                    tracing::info!(?backoff, "按指数退避等待后重试");
                    tokio::time::sleep(backoff).await;
                }
            }
        }
    }

    SwitchOutcome {
        ok,
        duration_ms: started.elapsed().as_millis() as i64,
        attempts,
        error: last_error,
        maa_log_excerpt: excerpt,
    }
}

/// 执行切号（§9.1 标准流程；INV-1 唯一入口）。
pub async fn run_switch(ctx: &SwitchCtx) -> Result<SwitchOutcome> {
    let account = &ctx.account;
    let device = &ctx.device;
    let holder = format!("switch:{}", account.id);

    // 亲和前提（§9.2）：仅对已预置的 (账号, 设备) 切号
    match ctx.store.login_status(&account.id, &device.name) {
        Ok(Some(status)) if status == "provisioned" => {}
        Ok(_) => {
            return Err(CoreError::Config(format!(
                "账号 {account_id} 未在设备 {device_name} 预置（MAA 只能选择该设备已登录过的账号，§9.2）：\
                 先运行 akops provision {account_id} --device {device_name}",
                account_id = account.id,
                device_name = device.name
            )));
        }
        Err(e) => return Err(CoreError::Other(e.to_string())),
    }

    // 设备租约（§6.5：手动 switch 也短暂持有）
    ctx.store
        .acquire_lease(&device.name, &holder)
        .map_err(|e| match e {
            StoreError::LeaseConflict { holder: h, .. } => CoreError::Other(format!(
                "设备 {} 被 {h} 占用：切号须独占设备（先停止会话或等待）",
                device.name
            )),
            other => CoreError::Other(other.to_string()),
        })?;
    let mut released = LeaseRelease {
        store: &ctx.store,
        device: device.name.clone(),
        holder: holder.clone(),
        done: false,
    };

    // 活跃会话须先 drain（M1 无调度器：只能来自手动 session start）
    if let Some(s) = ctx
        .store
        .active_session_by_device(&device.name)
        .map_err(|e| CoreError::Other(e.to_string()))?
    {
        return Err(CoreError::Config(format!(
            "设备 {} 上有活跃会话 #{}（{}）：请先停止该会话再切号",
            device.name, s.id, s.state
        )));
    }

    // 会话行（观测 + 审计；切换完成后进终态）
    let session_id = ctx
        .store
        .create_session(crate::store::NewSession {
            account_id: &account.id,
            device_name: &device.name,
            executor: "maa",
            runner: Some("process"),
            state: "switching",
            mower_port: None,
            slice_deadline_ms: None,
            max_runtime_deadline_ms: None,
        })
        .map_err(|e| CoreError::Other(e.to_string()))?;
    tracing::info!(account = %account.id, device = %device.name, session = session_id, "开始切号（INV-1: maa run startup）");

    let outcome = execute_switch(&SwitchPlan {
        wd: &ctx.wd,
        cfg: &ctx.cfg,
        account,
        device,
        maa_bin: &ctx.maa_bin,
        adb_bin: &ctx.adb_bin,
        timeout: ctx.timeout,
        backoff_initial_override: ctx.backoff_initial_override,
    })
    .await;
    let duration_ms = outcome.duration_ms;
    let ok = outcome.ok;
    let attempts = outcome.attempts;
    let last_error = outcome.error.clone();
    let excerpt = outcome.maa_log_excerpt.clone();

    // 记 switch_log + 会话终态
    let _ = ctx.store.insert_switch_log(&SwitchLogEntry {
        ts_ms: crate::store::now_ms(),
        account_id: account.id.clone(),
        device_name: device.name.clone(),
        ok,
        duration_ms,
        retries: attempts.saturating_sub(1),
        maa_log_excerpt: excerpt,
    });
    if ok {
        let _ = ctx.store.finish_session(
            session_id,
            "finished",
            "completed",
            None,
            "switch_succeeded",
        );
        tracing::info!(account = %account.id, device = %device.name, "切号成功（耗时 {duration_ms}ms）");
    } else {
        let _ = ctx.store.finish_session(
            session_id,
            "failed",
            "switch_failure",
            last_error.as_deref(),
            "switch_failed",
        );
        tracing::error!(account = %account.id, device = %device.name, "切号失败（重试 {attempts} 次耗尽）");
    }
    released.done = true;
    let _ = ctx.store.release_lease(&device.name, &holder);
    Ok(outcome)
}

/// RAII 兜底：异常路径也必须尝试释放租约。
struct LeaseRelease<'a> {
    store: &'a Store,
    device: String,
    holder: String,
    done: bool,
}

impl Drop for LeaseRelease<'_> {
    fn drop(&mut self) {
        if !self.done {
            let _ = self.store.release_lease(&self.device, &self.holder);
        }
    }
}

/// 退避延迟：initial × factor^(attempt-1)，封顶 max（§10.2）。
pub fn backoff_delay(
    backoff: &crate::config::BackoffConfig,
    override_initial: Option<Duration>,
    attempt: u32,
) -> Duration {
    if let Some(d) = override_initial {
        return d;
    }
    let initial = backoff.initial.0.as_millis() as f64;
    let capped =
        (initial * backoff.factor.powi(attempt as i32 - 1)).min(backoff.max.0.as_millis() as f64);
    Duration::from_millis(capped as u64)
}

/// 运行 `maa run startup -p default --batch`（INV-1）；成功返回输出摘录。
async fn run_maa_startup(
    maa_bin: &Path,
    timeout: Duration,
    maa_dir: &Path,
) -> std::result::Result<String, String> {
    let mut cmd = tokio::process::Command::new(maa_bin);
    cmd.args(["run", "startup", "-p", "default", "--batch"])
        .env("MAA_CONFIG_DIR", maa_dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    tracing::debug!(maa_dir = %maa_dir.display(), "spawn maa run startup");

    let child = cmd
        .spawn()
        .map_err(|e| format!("spawn {} 失败：{e}", maa_bin.display()))?;
    let output = match tokio::time::timeout(timeout, child.wait_with_output()).await {
        Ok(Ok(out)) => out,
        Ok(Err(e)) => return Err(format!("等待 maa 退出失败：{e}")),
        Err(_) => return Err(format!("切号超时（>{timeout:?}）：MAA 任务被终止")),
    };
    let combined = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if !output.status.success() {
        return Err(format!(
            "maa run startup 退出码 {:?}；输出尾部：{}",
            output.status.code(),
            tail_lines(&combined, 15)
        ));
    }
    Ok(tail_lines(&combined, 200))
}

/// 物化 MAA 配置：profiles/default.toml + tasks/startup.toml（§11.3；用户自定义任务不触碰）。
fn materialize_maa(
    maa_dir: &Path,
    account: &crate::model::Account,
    endpoints: &crate::device::DeviceEndpoints,
) -> Result<()> {
    let profiles = maa_dir.join("profiles");
    let tasks = maa_dir.join("tasks");
    std::fs::create_dir_all(&profiles).map_err(|e| CoreError::io(&profiles, e))?;
    std::fs::create_dir_all(&tasks).map_err(|e| CoreError::io(&tasks, e))?;

    let profile = render_profile(account, endpoints, RunnerKind::Process)?;
    let profile_path = profiles.join("default.toml");
    std::fs::write(&profile_path, &profile).map_err(|e| CoreError::io(&profile_path, e))?;

    let task = render_startup_task(account)?;
    let task_path = tasks.join("startup.toml");
    std::fs::write(&task_path, &task).map_err(|e| CoreError::io(&task_path, e))?;
    Ok(())
}

async fn adb_shell(
    adb_bin: &Path,
    serial: &str,
    args: &[&str],
) -> std::result::Result<String, String> {
    let out = tokio::process::Command::new(adb_bin)
        .args(["-s", serial])
        .args(args)
        .output()
        .await
        .map_err(|e| format!("spawn adb 失败：{e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(format!(
            "adb {args:?} 失败：{}",
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

/// 取文本最后 `n` 行。
fn tail_lines(text: &str, n: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.len().saturating_sub(n);
    lines[start..].join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Account, DeviceBackendKind, DeviceConnection, Server};

    fn account(id: &str, name: &str) -> Account {
        Account {
            id: id.into(),
            display_name: id.into(),
            server: Server::Official,
            account_name: name.into(),
            enabled: true,
            schedule: Default::default(),
            provisioned_on: vec![],
        }
    }

    fn device() -> Device {
        Device {
            name: "d1".into(),
            backend: DeviceBackendKind::External,
            connection: DeviceConnection {
                host_adb: "127.0.0.1:1".into(),
                docker_adb: None,
                docker_network: None,
            },
            notes: String::new(),
        }
    }

    fn ctx_with(account: &Account, store: std::sync::Arc<Store>) -> (tempfile::TempDir, SwitchCtx) {
        let tmp = tempfile::tempdir().unwrap();
        let wd = Workdir::new(tmp.path());
        let cfg = AkopsConfig::default();
        let mut ctx = SwitchCtx::new(&wd, &cfg, store, account.clone(), device());
        ctx.maa_bin = PathBuf::from("definitely-not-maa-xyz");
        ctx.backoff_initial_override = Some(Duration::ZERO);
        ctx.timeout = Duration::from_secs(5);
        (tmp, ctx)
    }

    #[tokio::test]
    async fn rejects_unprovisioned_pair() {
        let store = std::sync::Arc::new(Store::open_in_memory().unwrap());
        let acc = account("main", "1***2");
        let (_tmp, ctx) = ctx_with(&acc, store);
        let err = run_switch(&ctx).await.unwrap_err();
        assert!(err.to_string().contains("provision"), "{err}");
    }

    #[tokio::test]
    async fn fails_after_retries_when_maa_missing_and_records() {
        let store = std::sync::Arc::new(Store::open_in_memory().unwrap());
        let acc = account("main", "1***2");
        store.record_login("main", "d1", "provisioned").unwrap();
        let (_tmp, ctx) = ctx_with(&acc, store.clone());
        // 无活跃会话
        let out = run_switch(&ctx).await.unwrap();
        assert!(!out.ok);
        assert_eq!(out.attempts, 3, "1 + max_switch_retries(2) 次尝试");
        assert!(out.error.as_deref().unwrap_or_default().contains("spawn"));

        // switch_log 与会话终态均已记录
        let sessions = store.list_sessions(10).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].state, "failed");
        assert_eq!(sessions[0].outcome.as_deref(), Some("switch_failure"));
        // 租约已释放
        assert!(store.lease_holder("d1").unwrap().is_none());
    }

    #[tokio::test]
    async fn lease_conflict_blocks_switch() {
        let store = std::sync::Arc::new(Store::open_in_memory().unwrap());
        let acc = account("main", "1***2");
        store.record_login("main", "d1", "provisioned").unwrap();
        let (_tmp, ctx) = ctx_with(&acc, store.clone());
        store.acquire_lease("d1", "session:99").unwrap();
        let err = run_switch(&ctx).await.unwrap_err();
        assert!(err.to_string().contains("session:99"), "{err}");
        assert!(
            store.list_sessions(10).unwrap().is_empty(),
            "未进入流程不应建会话"
        );
    }

    #[tokio::test]
    async fn active_session_blocks_switch() {
        let store = std::sync::Arc::new(Store::open_in_memory().unwrap());
        let acc = account("main", "1***2");
        store.record_login("main", "d1", "provisioned").unwrap();
        let (_tmp, ctx) = ctx_with(&acc, store.clone());
        // 活跃会话（无租约行：如 daemon 崩溃后仅剩会话记录的残态）
        store
            .create_session(crate::store::NewSession {
                account_id: "other",
                device_name: "d1",
                executor: "mower",
                runner: Some("process"),
                state: "running",
                mower_port: None,
                slice_deadline_ms: None,
                max_runtime_deadline_ms: None,
            })
            .unwrap();
        let err = run_switch(&ctx).await.unwrap_err();
        assert!(err.to_string().contains("活跃会话"), "{err}");
    }

    #[test]
    fn materialize_writes_profile_and_task() {
        let tmp = tempfile::tempdir().unwrap();
        let maa_dir = tmp.path().join("maa");
        let endpoints = crate::device::DeviceEndpoints {
            host_adb: "127.0.0.1:2771".into(),
            docker_adb: None,
            docker_network: None,
        };
        materialize_maa(&maa_dir, &account("main", "1***2"), &endpoints).unwrap();
        let profile = std::fs::read_to_string(maa_dir.join("profiles/default.toml")).unwrap();
        assert!(profile.contains(r#"address = "127.0.0.1:2771""#));
        let task = std::fs::read_to_string(maa_dir.join("tasks/startup.toml")).unwrap();
        assert!(task.contains(r#"account_name = "1***2""#));
        // 幂等
        materialize_maa(&maa_dir, &account("main", "1***2"), &endpoints).unwrap();
    }

    #[test]
    fn backoff_grows_and_caps() {
        let tmp = tempfile::tempdir().unwrap();
        let wd = Workdir::new(tmp.path());
        let mut cfg = AkopsConfig::default();
        cfg.scheduler.backoff.initial = crate::config::HumanDuration::parse("5m").unwrap();
        cfg.scheduler.backoff.max = crate::config::HumanDuration::parse("60m").unwrap();
        cfg.scheduler.backoff.factor = 2.0;
        let store = std::sync::Arc::new(Store::open_in_memory().unwrap());
        let acc = account("a", "x");
        let ctx = SwitchCtx::new(&wd, &cfg, store, acc, device());
        let b = &ctx.cfg.scheduler.backoff;
        assert_eq!(backoff_delay(b, None, 1), Duration::from_secs(300));
        assert_eq!(backoff_delay(b, None, 2), Duration::from_secs(600));
        assert_eq!(
            backoff_delay(b, None, 9),
            Duration::from_secs(3600),
            "封顶 max"
        );
    }

    #[test]
    fn tail_lines_works() {
        let text = "a\nb\nc";
        assert_eq!(tail_lines(text, 2), "b\nc");
        assert_eq!(tail_lines(text, 10), "a\nb\nc");
    }
}
