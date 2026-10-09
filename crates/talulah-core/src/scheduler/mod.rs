//! 调度器（§10）：策略引擎 → 会话队列 → 设备租约 → 生命周期管理。
//!
//! M1 = 单设备串行 + 静态时间窗（设计文档 §10.1）：
//! - 就绪判定（§10.3）：enabled ∧ 处于时间窗 ∧ 无活跃会话 ∧ 未在退避
//! - 队列：`(priority desc, 就绪等待时长 desc)`，同优先级 FIFO
//! - 双超时 + 看门狗 + 退避：见 [`session`] 与 §10.2
//! - 退避状态为 daemon 内存态（重启清零，事件经 session_events 留痕）
//! - `daily_guarantee` M1 仅告警（游戏日内有窗口但零会话时记录日志）
//!
//! 引擎为单任务主循环：选号 → [`session::run_session_flow`]（内联 await）→
//! 退避记账 → 下一轮。多设备池（M2）将改为 per-device worker。

pub mod session;
pub mod window;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::watch;

use crate::config::{AkopsConfig, Workdir};
use crate::model::{Account, Device, ExecutorKind, ScheduledExecutor};
use crate::store::Store;

pub use session::{ActiveSlot, ActiveStop, ExecutorFactory, SessionRunResult, StopReason};

/// 引擎与 server 共享的运行依赖（手动会话发起也经由它，保证单写者与同一工厂）。
pub struct EngineShared {
    pub wd: Workdir,
    pub cfg: AkopsConfig,
    pub store: Arc<Store>,
    pub device: Device,
    pub factory: ExecutorFactory,
    /// 切号单次尝试超时
    pub switch_timeout: Duration,
    /// maa-cli 可执行文件（默认 PATH 中的 `maa`；测试注入假脚本）
    pub maa_bin: std::path::PathBuf,
    /// 测试用：覆盖调度退避初始间隔（None 用配置值）
    pub backoff_override: Option<Duration>,
}

/// 单设备调度引擎的依赖集（全部可注入以便测试）。
pub struct EngineDeps {
    pub shared: Arc<EngineShared>,
    /// 主循环空转轮询间隔
    pub tick: Duration,
}

/// 事件总线：会话状态迁移/选号等关键事件（WS `?events=1` 消费）。
#[derive(Clone)]
pub struct EventBus(pub Arc<tokio::sync::broadcast::Sender<String>>);

impl EventBus {
    pub fn new() -> EventBus {
        let (tx, _) = tokio::sync::broadcast::channel(256);
        EventBus(Arc::new(tx))
    }
    /// 事件广播（无订阅者时静默）。
    pub fn emit(&self, event: &str, detail: &serde_json::Value) {
        let msg = serde_json::json!({
            "ts_ms": crate::store::now_ms(),
            "event": event,
            "detail": detail,
        });
        let _ = self.0.send(msg.to_string());
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

/// 引擎控制句柄（server / 测试持有）。
#[derive(Clone)]
pub struct EngineHandle {
    /// 暂停自动调度（运行中会话不受影响，§15 schedule pause）
    pub pause: Arc<std::sync::atomic::AtomicBool>,
    /// 关停信号
    pub stop_tx: watch::Sender<bool>,
    /// 活跃会话槽（手动 drain 入口）
    pub active: ActiveSlot,
    /// 事件总线（WS events 消费）
    pub events: EventBus,
}

impl EngineHandle {
    pub fn new() -> (watch::Receiver<bool>, EngineHandle) {
        let (stop_tx, stop_rx) = watch::channel(false);
        (
            stop_rx,
            EngineHandle {
                pause: Arc::new(std::sync::atomic::AtomicBool::new(false)),
                stop_tx,
                active: ActiveSlot::default(),
                events: EventBus::new(),
            },
        )
    }

    pub fn is_paused(&self) -> bool {
        self.pause.load(std::sync::atomic::Ordering::Relaxed)
    }
}

impl Default for EngineHandle {
    fn default() -> Self {
        Self::new().1
    }
}

/// 退避记账（内存态，§10.2）。
#[derive(Debug, Clone)]
struct BackoffState {
    consecutive_failures: u32,
    until: tokio::time::Instant,
}

/// 引擎运行态（跨轮次记忆）。
#[derive(Default)]
struct EngineState {
    backoff: HashMap<String, BackoffState>,
    ready_since: HashMap<String, tokio::time::Instant>,
    /// 已告警的 (游戏日键, 账号)——daily_guarantee 每组合只告警一次
    guarantee_warned: std::collections::HashSet<(String, String)>,
}

/// 启动前的崩溃恢复（§10.2：daemon 崩溃后重启）。
/// 单写者（D7）：daemon 持锁启动时，**任何现存租约必属已死进程**——全部清收；
/// 非终态会话标记中断（outcome=executor_crash）。
pub fn recover_on_start(store: &Store) -> (usize, usize) {
    let orphans = store.recover_all_leases().unwrap_or_default();
    let mut interrupted = 0usize;
    if let Ok(rows) = store.list_sessions(500) {
        for s in rows {
            if matches!(
                s.state.as_str(),
                "created" | "queued" | "switching" | "running" | "draining"
            ) {
                let _ = store.finish_session(
                    s.id,
                    "failed",
                    "executor_crash",
                    Some("daemon 重启，会话中断恢复"),
                    "recovered_interrupted",
                );
                interrupted += 1;
            }
        }
    }
    if !orphans.is_empty() {
        tracing::warn!("回收孤儿租约：{}", orphans.join(", "));
    }
    if interrupted > 0 {
        tracing::warn!("标记中断会话 {interrupted} 个");
    }
    (orphans.len(), interrupted)
}

/// 引擎主循环（阻塞至 stop 信号；server 与测试直接 tokio::spawn）。
pub async fn run_engine(
    deps: EngineDeps,
    handle: EngineHandle,
    mut stop_rx: watch::Receiver<bool>,
) {
    tracing::info!(device = %deps.shared.device.name, "调度引擎启动（单设备 M1）");
    recover_on_start(&deps.shared.store);
    let mut state = EngineState::default();

    loop {
        // 先干活后节流：启动即尝试调度；停止信号在循环末尾的 select 中及时响应
        if *stop_rx.borrow() {
            break;
        }
        if handle.is_paused() {
            tokio::select! {
                res = stop_rx.changed() => {
                    if res.is_err() || *stop_rx.borrow() { break; }
                }
                _ = tokio::time::sleep(deps.tick) => {}
            }
            continue;
        }

        // 单设备互斥：设备上有活跃会话（例如手动 session start）则让路
        if let Ok(Some(s)) = deps
            .shared
            .store
            .active_session_by_device(&deps.shared.device.name)
        {
            tracing::debug!(session = s.id, "设备被会话占用，让路");
            continue;
        }

        let accounts = match deps.shared.wd.load_all_accounts() {
            Ok(a) => a,
            Err(e) => {
                tracing::error!("读取账号失败：{e}");
                tokio::time::sleep(deps.tick).await;
                continue;
            }
        };
        let now = chrono::Local::now();
        let candidate = pick_next(&deps, &mut state, &accounts, now);
        let Some((account, executor_kind, task, slice)) = candidate else {
            check_daily_guarantee(&deps, &mut state, &accounts, now);
            tokio::select! {
                res = stop_rx.changed() => {
                    if res.is_err() || *stop_rx.borrow() { break; }
                }
                _ = tokio::time::sleep(deps.tick) => {}
            }
            continue;
        };
        state.ready_since.remove(&account.id);

        tracing::info!(
            account = %account.id,
            ?executor_kind,
            ?task,
            "选中账号开会话（priority={}）", account.schedule.priority
        );
        handle.events.emit(
            "session_selected",
            &serde_json::json!({"account_id": account.id, "executor": match executor_kind {
                ExecutorKind::Mower => "mower",
                ExecutorKind::Maa => "maa",
            }}),
        );
        // 会话跑在独立 task：引擎才能在 stop 信号到达时注入关停并等它优雅收尾
        // （内联 await 会死锁：引擎阻塞期间无人向活跃会话发停止信号）
        let mut session_task = {
            let wd = deps.shared.wd.clone();
            let cfg = deps.shared.cfg.clone();
            let store = deps.shared.store.clone();
            let device = deps.shared.device.clone();
            let factory = deps.shared.factory.clone();
            let active = handle.active.clone();
            let account = account.clone();
            let maa_bin = deps.shared.maa_bin.clone();
            let switch_timeout = deps.shared.switch_timeout;
            let events = handle.events.clone();
            tokio::spawn(async move {
                session::run_session_flow(
                    &wd,
                    &cfg,
                    &store,
                    &device,
                    &account,
                    executor_kind,
                    task,
                    slice,
                    &factory,
                    &active,
                    switch_timeout,
                    &maa_bin,
                    &events,
                )
                .await
            })
        };
        let result = tokio::select! {
            r = &mut session_task => r.unwrap_or_else(|e| session::SessionRunResult {
                session_id: 0,
                ok: false,
                outcome: crate::model::SessionOutcome::ExecutorCrash,
                error: Some(format!("会话 task panic：{e}")),
            }),
            res = stop_rx.changed() => {
                if res.is_err() || *stop_rx.borrow() {
                    handle.active.request_stop_any(StopReason::Shutdown);
                }
                let grace = Duration::from_millis(
                    deps.shared.cfg.scheduler.drain_grace.0.as_millis() as u64,
                ) + Duration::from_secs(10);
                match tokio::time::timeout(grace, &mut session_task).await {
                    Ok(Ok(r)) => r,
                    _ => session::SessionRunResult {
                        session_id: 0,
                        ok: false,
                        outcome: crate::model::SessionOutcome::ExecutorCrash,
                        error: Some("关停等待会话收尾超时".into()),
                    },
                }
            }
        };

        // 退避记账（§10.2）：失败指数退避；成功清零
        let b = &deps.shared.cfg.scheduler.backoff;
        if result.ok && matches!(result.outcome, crate::model::SessionOutcome::Completed) {
            state.backoff.remove(&account.id);
        }
        let is_failure = !result.ok
            || matches!(
                result.outcome,
                crate::model::SessionOutcome::Watchdog
                    | crate::model::SessionOutcome::ExecutorCrash
                    | crate::model::SessionOutcome::SwitchFailure
            );
        if is_failure {
            let entry = state
                .backoff
                .entry(account.id.clone())
                .or_insert(BackoffState {
                    consecutive_failures: 0,
                    until: tokio::time::Instant::now(),
                });
            entry.consecutive_failures += 1;
            let delay = crate::switch::backoff_delay(
                b,
                deps.shared.backoff_override,
                entry.consecutive_failures,
            );
            entry.until = tokio::time::Instant::now() + delay;
            tracing::warn!(
                account = %account.id,
                failures = entry.consecutive_failures,
                ?delay,
                outcome = result.outcome.name(),
                "账号进入退避"
            );
        } else if result.ok {
            state.backoff.remove(&account.id);
        }

        // 会话后的空调节流（保持停止响应性）
        tokio::select! {
            res = stop_rx.changed() => {
                if res.is_err() || *stop_rx.borrow() { break; }
            }
            _ = tokio::time::sleep(deps.tick) => {}
        }
    }

    // 关停：对活跃会话注入 Shutdown（runner 优雅 drain 后退出）
    handle.active.request_stop_any(StopReason::Shutdown);
    // 给 runner 一点收尾时间
    for _ in 0..50 {
        if handle
            .active
            .0
            .lock()
            .expect("active 锁 poisoned")
            .is_none()
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    tracing::info!("调度引擎已停止");
}

/// 就绪判定 + 队列选择（§10.3）：`(priority desc, 就绪等待时长 desc)`。
fn pick_next(
    deps: &EngineDeps,
    state: &mut EngineState,
    accounts: &[Account],
    now: chrono::DateTime<chrono::Local>,
) -> Option<(Account, ExecutorKind, Option<String>, Duration)> {
    struct Candidate {
        account: Account,
        executor_kind: ExecutorKind,
        task: Option<String>,
        slice: Duration,
        ready_since: tokio::time::Instant,
    }
    let mut best: Option<Candidate> = None;
    for account in accounts {
        if !account.enabled {
            continue;
        }
        if state
            .backoff
            .get(&account.id)
            .map(|b| b.until > tokio::time::Instant::now())
            .unwrap_or(false)
        {
            continue;
        }
        if deps
            .shared
            .store
            .active_session_by_account(&account.id)
            .ok()
            .flatten()
            .is_some()
        {
            continue;
        }
        let Some((_w, scheduled_executor, task)) =
            window::active_window(now, &account.schedule.windows)
        else {
            continue;
        };
        let executor_kind = match scheduled_executor {
            ScheduledExecutor::Mower => ExecutorKind::Mower,
            ScheduledExecutor::Maa => ExecutorKind::Maa,
        };
        // maa 窗口必须有任务名（模型校验兜底；手改文件绕过校验时跳过）
        if executor_kind == ExecutorKind::Maa && task.unwrap_or("").is_empty() {
            tracing::warn!(account = %account.id, "maa 窗口缺 task，跳过");
            continue;
        }
        let ready_since = *state
            .ready_since
            .entry(account.id.clone())
            .or_insert(tokio::time::Instant::now());
        let slice = account
            .schedule
            .slice
            .map(|d| Duration::from_millis(d.0.as_millis() as u64))
            .unwrap_or(Duration::from_millis(
                deps.shared.cfg.scheduler.default_slice.0.as_millis() as u64,
            ));
        let cand = Candidate {
            account: account.clone(),
            executor_kind,
            task: task.map(String::from),
            slice,
            ready_since,
        };
        best = Some(match best {
            None => cand,
            Some(b) => {
                // priority desc；同优先级：就绪更早（等待更久）优先
                if cand.account.schedule.priority > b.account.schedule.priority
                    || (cand.account.schedule.priority == b.account.schedule.priority
                        && cand.ready_since < b.ready_since)
                {
                    cand
                } else {
                    b
                }
            }
        });
    }
    best.map(|c| (c.account, c.executor_kind, c.task, c.slice))
}

/// daily_guarantee（M1 告警级）：游戏日内有窗口但零会话 → 记日志（每组合一次）。
fn check_daily_guarantee(
    deps: &EngineDeps,
    state: &mut EngineState,
    accounts: &[Account],
    now: chrono::DateTime<chrono::Local>,
) {
    if !deps.shared.cfg.scheduler.daily_guarantee {
        return;
    }
    let Some(day_key) = window::game_day_key(now, &deps.shared.cfg.scheduler.game_day_boundary)
    else {
        return;
    };
    let day_start = match window::game_day_start(now, &deps.shared.cfg.scheduler.game_day_boundary)
    {
        Some(s) => s.timestamp_millis(),
        None => return,
    };
    for account in accounts {
        if !account.enabled || account.schedule.windows.is_empty() {
            continue;
        }
        // 仅当日界之后的所有窗口都已结束（今天不再会开会话）时判定
        let all_ended = account
            .schedule
            .windows
            .iter()
            .all(|w| window::active_window(now, std::slice::from_ref(w)).is_none());
        if !all_ended {
            continue;
        }
        let key = (day_key.clone(), account.id.clone());
        if state.guarantee_warned.contains(&key) {
            continue;
        }
        let cnt = deps
            .shared
            .store
            .sessions_cnt_since(&account.id, day_start)
            .unwrap_or(0);
        if cnt == 0 {
            state.guarantee_warned.insert(key);
            tracing::warn!(
                account = %account.id,
                game_day = %day_key,
                "daily_guarantee：本游戏日有窗口但未运行任何会话（M1 仅告警，不补跑）"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::model::SessionState;

    #[test]
    fn session_state_lifecycle_table() {
        // 状态机迁移表驱动测试（§10.2 完整迁移表随 M2 多设备扩展）
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
