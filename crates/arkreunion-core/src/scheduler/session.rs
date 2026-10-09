//! 单会话生命周期（§10.2 状态机的执行侧）：Queued → Switching → Running →
//! Draining → 终态。由引擎（`super::run_engine`）在单设备 M1 下串行驱动。
//!
//! 双超时（slice/max_runtime）、看门狗（连续 N 次健康失败）与停止信号
//! （手动 drain / daemon 关停）在监控环内 `tokio::select!`。

use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::watch;

use super::EventBus;
use crate::config::{AkopsConfig, Workdir};
use crate::executor::{Executor, ExecutorHealth, SessionCtx};
use crate::model::{Device, ExecutorKind, RunnerKind, SessionOutcome};
use crate::store::{NewSession, Store};
use crate::switch::{SwitchPlan, execute_switch};

/// 引擎注入的执行器工厂（测试用 Fake 替换；INV-3）。
pub type ExecutorFactory =
    Arc<dyn Fn(ExecutorKind) -> Result<Box<dyn Executor>, String> + Send + Sync>;

/// 停止原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    /// 控制台/API 手动 drain
    Manual,
    /// daemon 关停
    Shutdown,
}

impl StopReason {
    pub fn as_str(self) -> &'static str {
        match self {
            StopReason::Manual => "manual",
            StopReason::Shutdown => "shutdown",
        }
    }
}

/// 引擎的活跃会话槽：注册 (session_id → stop 发送端)，供手动 drain 与关停注入。
#[derive(Clone, Default)]
pub struct ActiveSlot(pub Arc<Mutex<Option<ActiveStop>>>);

pub struct ActiveStop {
    pub session_id: u64,
    pub tx: watch::Sender<Option<StopReason>>,
}

impl ActiveSlot {
    /// 对指定会话注入停止信号；返回是否存在该活跃会话。
    pub fn request_stop(&self, session_id: u64, reason: StopReason) -> bool {
        let guard = self.0.lock().expect("active 锁 poisoned");
        match guard.as_ref() {
            Some(a) if a.session_id == session_id => {
                let _ = a.tx.send(Some(reason));
                true
            }
            _ => false,
        }
    }

    /// 对当前活跃会话（若有）注入关停信号。
    pub fn request_stop_any(&self, reason: StopReason) {
        if let Some(a) = self.0.lock().expect("active 锁 poisoned").as_ref() {
            let _ = a.tx.send(Some(reason));
        }
    }
}

/// 会话运行结果（引擎用于退避记账）。
#[derive(Debug, Clone)]
pub struct SessionRunResult {
    pub session_id: u64,
    pub ok: bool,
    pub outcome: SessionOutcome,
    pub error: Option<String>,
}

/// 运行一个完整会话（假定引擎已选定账号/设备/执行器/任务并持有引用）。
#[allow(clippy::too_many_arguments)]
pub async fn run_session_flow(
    wd: &Workdir,
    cfg: &AkopsConfig,
    store: &Arc<Store>,
    device: &Device,
    account: &crate::model::Account,
    executor_kind: ExecutorKind,
    task: Option<String>,
    slice: Duration,
    factory: &ExecutorFactory,
    active: &ActiveSlot,
    switch_timeout: Duration,
    maa_bin: &std::path::Path,
    events: &EventBus,
) -> SessionRunResult {
    // 亲和前提（§9.2）：调度切号只对已预置对有效
    match store.login_status(&account.id, &device.name) {
        Ok(Some(s)) if s == "provisioned" => {}
        _ => {
            let msg = format!(
                "账号 {} 未在设备 {} 预置，跳过自动调度（先 arkreunion provision）",
                account.id, device.name
            );
            tracing::warn!("{msg}");
            return SessionRunResult {
                session_id: 0,
                ok: false,
                outcome: SessionOutcome::SwitchFailure,
                error: Some(msg),
            };
        }
    }

    // 会话行 + 设备租约
    let session_id = match store.create_session(NewSession {
        account_id: &account.id,
        device_name: &device.name,
        executor: match executor_kind {
            ExecutorKind::Mower => "mower",
            ExecutorKind::Maa => "maa",
        },
        runner: Some("process"),
        state: "queued",
        mower_port: None,
        slice_deadline_ms: None,
        max_runtime_deadline_ms: None,
    }) {
        Ok(id) => id,
        Err(e) => {
            return failed(0, SessionOutcome::ExecutorCrash, e.to_string());
        }
    };
    let holder = format!("session:{session_id}");
    if let Err(e) = store.acquire_lease(&device.name, &holder) {
        let _ = store.finish_session(
            session_id,
            "cancelled",
            "cancelled",
            Some(&e.to_string()),
            "lease_conflict",
        );
        return failed(session_id, SessionOutcome::Cancelled, e.to_string());
    }
    let released = ReleaseOnDrop {
        store: store.clone(),
        device: device.name.clone(),
        holder: holder.clone(),
    };
    let _ = store.update_session_state(
        session_id,
        "switching",
        "switching",
        Some("INV-1: maa run startup"),
    );

    // ---- 切号（INV-1 唯一路径）----
    let adb_bin = cfg.paths.adb_path_expanded();
    let plan = SwitchPlan {
        wd,
        cfg,
        account,
        device,
        maa_bin,
        adb_bin: &adb_bin,
        timeout: switch_timeout,
        backoff_initial_override: None,
    };
    let switch_out = execute_switch(&plan).await;
    let _ = store.insert_switch_log(&crate::store::SwitchLogEntry {
        ts_ms: crate::store::now_ms(),
        account_id: account.id.clone(),
        device_name: device.name.clone(),
        ok: switch_out.ok,
        duration_ms: switch_out.duration_ms,
        retries: switch_out.attempts.saturating_sub(1),
        maa_log_excerpt: switch_out.maa_log_excerpt.clone(),
    });
    if !switch_out.ok {
        let err = switch_out.error.clone().unwrap_or_default();
        let _ = store.finish_session(
            session_id,
            "failed",
            "switch_failure",
            Some(&err),
            "switch_failed",
        );
        return failed(session_id, SessionOutcome::SwitchFailure, err);
    }

    // ---- 端口（mower）+ 执行器启动 ----
    let port = if executor_kind == ExecutorKind::Mower {
        match store.alloc_port("mower_webview", &holder, cfg.ports.mower_session_range) {
            Ok(p) => {
                let _ = store.set_mower_port(session_id, p);
                Some(p)
            }
            Err(e) => {
                let _ = store.finish_session(
                    session_id,
                    "failed",
                    "executor_crash",
                    Some(&e.to_string()),
                    "port_exhausted",
                );
                return failed(session_id, SessionOutcome::ExecutorCrash, e.to_string());
            }
        }
    } else {
        None
    };

    let ctx = SessionCtx {
        session_id,
        account: account.clone(),
        endpoints: crate::device::DeviceEndpoints {
            host_adb: device.connection.host_adb.clone(),
            docker_adb: device.connection.docker_adb.clone(),
            docker_network: device.connection.docker_network.clone(),
        },
        runner: RunnerKind::Process,
        mower_port: port,
        workdir: wd.root.clone(),
        mower_checkout: cfg.paths.mower_dir_expanded(),
        maa_task: task,
    };
    let executor = match factory(executor_kind) {
        Ok(e) => e,
        Err(e) => {
            let _ = store.finish_session(
                session_id,
                "failed",
                "executor_crash",
                Some(&e),
                "start_failed",
            );
            return failed(session_id, SessionOutcome::ExecutorCrash, e);
        }
    };
    let _ = store.update_session_state(session_id, "running", "executor_starting", None);
    let handle = match executor.start(&ctx).await {
        Ok(h) => h,
        Err(e) => {
            let _ = store.finish_session(
                session_id,
                "failed",
                "executor_crash",
                Some(&e.to_string()),
                "start_failed",
            );
            return failed(session_id, SessionOutcome::ExecutorCrash, e.to_string());
        }
    };
    let now = crate::store::now_ms();
    let slice_deadline = tokio::time::Instant::now() + slice;
    let max_deadline = tokio::time::Instant::now()
        + Duration::from_millis(cfg.scheduler.max_session_runtime.0.as_millis() as u64);
    let _ = store.set_executor_handle(
        session_id,
        &handle.locator,
        Some(now + slice.as_millis() as i64),
        Some(now + cfg.scheduler.max_session_runtime.0.as_millis() as i64),
    );
    tracing::info!(session = session_id, locator = %handle.locator, "会话运行中");
    events.emit(
        "session_running",
        &serde_json::json!({"account_id": account.id, "session_id": session_id, "mower_port": port}),
    );

    // ---- 监控环：双超时 + 看门狗 + 停止信号（§10.2）----
    let (stop_tx, mut stop_rx) = watch::channel::<Option<StopReason>>(None);
    {
        let mut slot = active.0.lock().expect("active 锁 poisoned");
        *slot = Some(ActiveStop {
            session_id,
            tx: stop_tx,
        });
    }
    let (final_state, outcome) = monitor(MonitorParams {
        session_id,
        executor: executor.as_ref(),
        handle: &handle,
        slice_deadline,
        max_deadline,
        watchdog_interval: Duration::from_millis(
            cfg.scheduler.watchdog_interval.0.as_millis() as u64
        )
        .max(Duration::from_millis(50)),
        watchdog_threshold: cfg.scheduler.watchdog_threshold,
        stop_rx: &mut stop_rx,
    })
    .await;
    // 撤销注册
    {
        let mut slot = active.0.lock().expect("active 锁 poisoned");
        if slot.as_ref().map(|a| a.session_id) == Some(session_id) {
            *slot = None;
        }
    }

    let _ = store.update_session_state(
        session_id,
        "draining",
        "drain",
        Some(match outcome {
            SessionOutcome::SliceExpired => "时间片到期",
            SessionOutcome::MaxRuntime => "硬上限",
            SessionOutcome::Watchdog => "看门狗",
            SessionOutcome::Cancelled => "手动/关停",
            _ => "其他",
        }),
    );
    if let Err(e) = executor
        .drain(
            &handle,
            Duration::from_millis(cfg.scheduler.drain_grace.0.as_millis() as u64),
        )
        .await
    {
        tracing::warn!(session = session_id, "drain 异常（已按强杀兜底）：{e}");
    }
    let _ = store.finish_session(
        session_id,
        final_state,
        outcome.name(),
        None,
        "session_ended",
    );
    let _ = store.release_ports(&holder);
    drop(released); // 显式释放租约（RAII 兜底异常路径）
    SessionRunResult {
        session_id,
        ok: true,
        outcome,
        error: None,
    }
}

/// 监控环入参。
struct MonitorParams<'a> {
    session_id: u64,
    executor: &'a dyn Executor,
    handle: &'a crate::executor::ExecutorHandle,
    slice_deadline: tokio::time::Instant,
    max_deadline: tokio::time::Instant,
    watchdog_interval: Duration,
    watchdog_threshold: u32,
    stop_rx: &'a mut watch::Receiver<Option<StopReason>>,
}

/// 监控环：返回 (终态 state, outcome)。
async fn monitor(p: MonitorParams<'_>) -> (&'static str, SessionOutcome) {
    let mut health_fails: u32 = 0;
    let mut health_tick = tokio::time::interval(p.watchdog_interval);
    health_tick.tick().await; // 跳过立即触发的首个 tick
    loop {
        tokio::select! {
            _ = tokio::time::sleep_until(p.slice_deadline) => {
                return ("finished", SessionOutcome::SliceExpired);
            }
            _ = tokio::time::sleep_until(p.max_deadline) => {
                return ("timed_out", SessionOutcome::MaxRuntime);
            }
            _ = health_tick.tick() => {
                match p.executor.health(p.handle).await {
                    ExecutorHealth::Alive => health_fails = 0,
                    other => {
                        health_fails += 1;
                        tracing::warn!(session = p.session_id, ?other, fails = health_fails, "执行器健康异常");
                        if health_fails >= p.watchdog_threshold {
                            return ("finished", SessionOutcome::Watchdog);
                        }
                    }
                }
            }
            res = p.stop_rx.changed() => {
                let reason = if res.is_err() { None } else { *p.stop_rx.borrow_and_update() };
                if let Some(reason) = reason {
                    tracing::info!(session = p.session_id, reason = reason.as_str(), "收到停止信号");
                    return ("finished", SessionOutcome::Cancelled);
                }
            }
        }
    }
}

fn failed(session_id: u64, outcome: SessionOutcome, error: String) -> SessionRunResult {
    SessionRunResult {
        session_id,
        ok: false,
        outcome,
        error: Some(error),
    }
}

/// RAII：异常路径释放租约；端口由显式路径释放。
struct ReleaseOnDrop {
    store: Arc<Store>,
    device: String,
    holder: String,
}

impl Drop for ReleaseOnDrop {
    fn drop(&mut self) {
        let _ = self.store.release_lease(&self.device, &self.holder);
    }
}
