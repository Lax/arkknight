//! 调度器全场景测试（§19：FakeExecutor 在 CI 无设备跑通调度）。
//!
//! 真实切号路径用假 maa 脚本（exit 0/1），执行器用 FakeExecutor；
//! 时间片/看门狗间隔压到 1s 级（HumanDuration 最小单位 s）。

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Duration;

use akops_core::config::{AkopsConfig, Workdir};
use akops_core::executor::{Executor, ExecutorHandle, ExecutorHealth, SessionCtx};
use akops_core::model::{
    Account, Device, DeviceBackendKind, DeviceConnection, ExecutorKind, RunnerKind, Server,
};
use akops_core::scheduler::{EngineDeps, EngineHandle, ExecutorFactory, run_engine};
use akops_core::store::Store;

/// Fake 执行器：health 由共享字决定（0=Alive，1=Dead），drain 立即成功。
struct FakeExecutor {
    health: Arc<AtomicU8>,
}

#[async_trait::async_trait]
impl Executor for FakeExecutor {
    fn kind(&self) -> ExecutorKind {
        ExecutorKind::Mower
    }
    async fn start(
        &self,
        ctx: &SessionCtx,
    ) -> Result<ExecutorHandle, akops_core::executor::ExecutorError> {
        Ok(ExecutorHandle {
            session_id: ctx.session_id,
            kind: self.kind(),
            runner: RunnerKind::Process,
            locator: "fake".into(),
        })
    }
    async fn drain(
        &self,
        _h: &ExecutorHandle,
        _g: Duration,
    ) -> Result<(), akops_core::executor::ExecutorError> {
        Ok(())
    }
    async fn health(&self, _h: &ExecutorHandle) -> ExecutorHealth {
        match self.health.load(Ordering::SeqCst) {
            0 => ExecutorHealth::Alive,
            _ => ExecutorHealth::Dead,
        }
    }
}

fn fake_maa(dir: &std::path::Path, exit_code: i32) -> std::path::PathBuf {
    let path = dir.join("fake-maa.sh");
    std::fs::write(
        &path,
        format!("#!/bin/sh\necho fake-maa\nexit {exit_code}\n"),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    path
}

fn test_cfg() -> AkopsConfig {
    let mut c = AkopsConfig::default();
    c.scheduler.default_slice = akops_core::config::HumanDuration::parse("1s").unwrap();
    c.scheduler.max_session_runtime = akops_core::config::HumanDuration::parse("60m").unwrap();
    c.scheduler.watchdog_interval = akops_core::config::HumanDuration::parse("1s").unwrap();
    c.scheduler.watchdog_threshold = 2;
    c.scheduler.drain_grace = akops_core::config::HumanDuration::parse("1s").unwrap();
    c.scheduler.backoff.initial = akops_core::config::HumanDuration::parse("1s").unwrap();
    c.scheduler.backoff.max = akops_core::config::HumanDuration::parse("2s").unwrap();
    c
}

fn account_with_window(id: &str, priority: u8) -> Account {
    Account {
        id: id.into(),
        display_name: id.into(),
        server: Server::Official,
        account_name: format!("{id}-138****0000"),
        uid: None,
        enabled: true,
        schedule: akops_core::model::AccountSchedule {
            // 全天窗口：任何测试时刻都就绪
            windows: vec![akops_core::model::TimeWindow {
                start: "00:00".into(),
                end: "23:59".into(),
                executor: akops_core::model::ScheduledExecutor::Mower,
                task: None,
            }],
            priority,
            slice: None,
        },
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

struct Bench {
    dir: tempfile::TempDir,
    wd: Workdir,
    store: Arc<Store>,
}

fn bench(accounts: &[Account]) -> Bench {
    let dir = tempfile::tempdir().unwrap();
    let wd = Workdir::init(dir.path(), &test_cfg()).unwrap();
    let store = Arc::new(Store::open(&wd.db_path()).unwrap());
    for a in accounts {
        wd.save_account(a).unwrap();
        store.record_login(&a.id, "d1", "provisioned").unwrap();
    }
    Bench { dir, wd, store }
}

fn engine_deps_with(
    b: &Bench,
    factory: ExecutorFactory,
    maa: &std::path::Path,
    adjust: impl FnOnce(&mut AkopsConfig),
) -> EngineDeps {
    let mut cfg = b.wd.load_config().unwrap();
    adjust(&mut cfg);
    EngineDeps {
        shared: Arc::new(akops_core::scheduler::EngineShared {
            wd: b.wd.clone(),
            cfg,
            store: b.store.clone(),
            device: device(),
            factory,
            switch_timeout: Duration::from_secs(5),
            maa_bin: maa.to_path_buf(),
            backoff_override: None,
        }),
        tick: Duration::from_millis(100),
    }
}

fn engine_deps(b: &Bench, factory: ExecutorFactory, maa: &std::path::Path) -> EngineDeps {
    engine_deps_with(b, factory, maa, |_| {})
}

fn alive_factory() -> ExecutorFactory {
    Arc::new(|_| {
        Ok(Box::new(FakeExecutor {
            health: Arc::new(AtomicU8::new(0)),
        }) as Box<dyn Executor>)
    })
}

/// 启动引擎，运行 `run_for` 后停止（会话终态由测试直接查 store）。
async fn run_for_then_stop(
    deps: EngineDeps,
    handle: &EngineHandle,
    stop_rx: tokio::sync::watch::Receiver<bool>,
    run_for: Duration,
    pause: bool,
) {
    if pause {
        handle
            .pause
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }
    let engine = tokio::spawn(run_engine(deps, handle.clone(), stop_rx));
    tokio::time::sleep(run_for).await;
    let _ = handle.stop_tx.send(true);
    let _ = tokio::time::timeout(Duration::from_secs(15), engine).await;
}

#[tokio::test]
async fn rotation_two_accounts_alternate_and_finish() {
    let accounts = [account_with_window("a", 50), account_with_window("b", 50)];
    let b = bench(&accounts);
    let maa = fake_maa(b.dir.path(), 0);
    let deps = engine_deps(&b, alive_factory(), &maa);
    let store = b.store.clone();
    let (stop_rx, handle) = EngineHandle::new();
    run_for_then_stop(deps, &handle, stop_rx, Duration::from_millis(4500), false).await;
    let rows = store.list_sessions(50).unwrap();
    let finished: Vec<_> = rows.iter().filter(|r| r.state == "finished").collect();
    assert!(finished.len() >= 2, "应完成 ≥2 个会话，实际 {rows:?}");
    // 停止瞬间的最后一个会话以 cancelled 收尾属正常（stop 竞态），其余应 slice_expired
    assert!(
        finished
            .iter()
            .filter(|r| r.outcome.as_deref() == Some("slice_expired"))
            .count()
            >= 2,
        "应 ≥2 个时间片到期会话：{finished:?}"
    );
    if finished.len() >= 2 {
        assert_ne!(
            finished[0].account_id, finished[1].account_id,
            "同优先级应轮转交替：{finished:?}"
        );
    }
    assert_eq!(store.lease_holder("d1").unwrap(), None, "租约应全部释放");
}

#[tokio::test]
async fn switch_failure_records_and_limits_rate() {
    let accounts = [account_with_window("a", 50)];
    let b = bench(&accounts);
    let maa = fake_maa(b.dir.path(), 1); // 切号必败
    let deps = engine_deps(&b, alive_factory(), &maa);
    let store = b.store.clone();
    let (stop_rx, handle) = EngineHandle::new();
    run_for_then_stop(deps, &handle, stop_rx, Duration::from_millis(3000), false).await;
    let rows = store.list_sessions(50).unwrap();
    assert!(!rows.is_empty(), "应有失败会话记录");
    assert!(
        rows.iter()
            .all(|r| r.outcome.as_deref() == Some("switch_failure") && r.state == "failed"),
        "全部为切号失败终态：{rows:?}"
    );
    // 退避生效：3s 内失败会话数受限（切号内部重试 + 引擎退避）
    assert!(rows.len() <= 2, "退避应限制重试频率：{}", rows.len());
}

#[tokio::test]
async fn watchdog_dead_executor_terminates_session() {
    let accounts = [account_with_window("a", 50)];
    let b = bench(&accounts);
    let maa = fake_maa(b.dir.path(), 0);
    let dead_factory: ExecutorFactory = Arc::new(|_| {
        Ok(Box::new(FakeExecutor {
            health: Arc::new(AtomicU8::new(1)),
        }) as Box<dyn Executor>)
    });
    // slice 加大：让看门狗（2×1s）先于时间片触发
    let deps = engine_deps_with(&b, dead_factory, &maa, |c| {
        c.scheduler.default_slice = akops_core::config::HumanDuration::parse("5s").unwrap();
    });
    let store = b.store.clone();
    let (stop_rx, handle) = EngineHandle::new();
    run_for_then_stop(deps, &handle, stop_rx, Duration::from_millis(4000), false).await;
    let rows = store.list_sessions(50).unwrap();
    assert!(
        rows.iter()
            .any(|r| r.outcome.as_deref() == Some("watchdog")),
        "应有看门狗收尾会话：{rows:?}"
    );
}

#[tokio::test]
async fn pause_prevents_new_sessions() {
    let accounts = [account_with_window("a", 50)];
    let b = bench(&accounts);
    let maa = fake_maa(b.dir.path(), 0);
    let deps = engine_deps(&b, alive_factory(), &maa);
    let store = b.store.clone();
    let (stop_rx, handle) = EngineHandle::new();
    run_for_then_stop(deps, &handle, stop_rx, Duration::from_millis(1500), true).await;
    assert!(
        store.list_sessions(50).unwrap().is_empty(),
        "暂停期间不应有会话"
    );
}

#[tokio::test]
async fn shutdown_drains_active_session() {
    let accounts = [account_with_window("a", 50)];
    let b = bench(&accounts);
    let maa = fake_maa(b.dir.path(), 0);
    // 长时间片：会话运行中被关停信号打断
    let deps = engine_deps_with(&b, alive_factory(), &maa, |c| {
        c.scheduler.default_slice = akops_core::config::HumanDuration::parse("60m").unwrap();
    });
    let store = b.store.clone();
    let (stop_rx, handle) = EngineHandle::new();
    run_for_then_stop(deps, &handle, stop_rx, Duration::from_millis(2500), false).await;
    let rows = store.list_sessions(50).unwrap();
    assert!(
        rows.iter()
            .any(|r| r.outcome.as_deref() == Some("cancelled")),
        "关停应优雅 drain 活跃会话：{rows:?}"
    );
}
