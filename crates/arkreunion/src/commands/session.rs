//! `arkreunion session`：手动会话管理（§15；M1 无调度器，插队语义=手动独占）。
//!
//! start：租约 + 端口分配 + conf 白名单 patch + 执行器启动 + 日志归档；
//! stop：POST /stop（mower）→ grace → kill 兜底 → 释放端口/租约 → 终态。
//! CLI 一次性进程，stop 在新进程里按 locator（pid/port）操作。

use std::time::Duration;

use anyhow::{Result, bail};
use clap::Subcommand;
use std::io::Seek;

use arkreunion_core::config::Workdir;
use arkreunion_core::executor::mower::{parse_locator, stop_via_http};
use arkreunion_core::executor::{MaaCliExecutor, MowerProcessExecutor, SessionCtx};
use arkreunion_core::lock::WorkdirGuard;
use arkreunion_core::model::ExecutorKind;
use arkreunion_core::store::SessionRow;
use arkreunion_core::{config::AkopsConfig, store::Store};
use std::sync::Arc;

#[derive(Subcommand, Debug)]
pub enum SessionCmd {
    /// 手动开会话（独占设备；自动调度的插队语义待任务 7）
    Start {
        account: String,
        /// 执行器（mower | maa）
        #[arg(long, default_value = "mower")]
        executor: String,
        /// 时间片（如 90m；缺省用全局默认，到期由任务 7 调度器执行）
        #[arg(long)]
        slice: Option<String>,
        /// MAA 任务名（仅 --executor maa；来自 accounts/<id>/maa/tasks/）
        #[arg(long)]
        task: Option<String>,
    },
    /// 停止会话（优雅 + 超时强杀）
    Stop {
        /// 会话 id（与 --account 二选一）
        id: Option<u64>,
        /// 停止该账号的活跃会话
        #[arg(long)]
        account: Option<String>,
        /// 优雅等待（默认 30s）
        #[arg(long)]
        grace: Option<String>,
    },
    /// 列出会话（最近优先）
    List {
        #[arg(long, default_value_t = 20)]
        limit: u32,
    },
    /// 查看会话日志（logs/sessions/<id>.log）
    Logs {
        id: u64,
        /// 跟踪输出（Ctrl+C 结束）
        #[arg(short = 'f')]
        follow: bool,
    },
}

const MANUAL_STOP_GRACE: Duration = Duration::from_secs(30);

pub(crate) async fn run(wd: Workdir, cmd: SessionCmd) -> Result<()> {
    match cmd {
        SessionCmd::List { limit } => list(&wd, limit).await,
        SessionCmd::Logs { id, follow } => logs(&wd, id, follow).await,
        _ => {
            // 写类：锁 + 库
            let guard = crate::commands::write_lock(&wd)?;
            let store = crate::commands::open_store(&wd)?;
            crate::commands::recover_orphans(&store);
            match cmd {
                SessionCmd::Start {
                    account,
                    executor,
                    slice,
                    task,
                } => start(wd, guard, store, account, executor, slice, task).await,
                SessionCmd::Stop { id, account, grace } => {
                    stop(wd, guard, store, id, account, grace).await
                }
                _ => unreachable!("读类已在上方处理"),
            }
        }
    }
}

fn python_bin() -> &'static str {
    if cfg!(windows) { "python" } else { "python3" }
}

fn build_executor(kind: ExecutorKind) -> Box<dyn arkreunion_core::executor::Executor> {
    match kind {
        ExecutorKind::Mower => Box::new(MowerProcessExecutor::new(python_bin())),
        ExecutorKind::Maa => Box::new(MaaCliExecutor::new("maa")),
    }
}

async fn start(
    wd: Workdir,
    _guard: WorkdirGuard,
    store: Arc<Store>,
    account_key: String,
    executor_name: String,
    slice: Option<String>,
    task: Option<String>,
) -> Result<()> {
    let cfg: AkopsConfig = wd.load_config()?;
    let account = wd.load_account(&account_key)?;
    let dev = crate::commands::resolve_device(&wd, None)?;

    let kind = match executor_name.as_str() {
        "mower" => ExecutorKind::Mower,
        "maa" => ExecutorKind::Maa,
        other => bail!("executor {other:?} 不合法：mower|maa"),
    };
    if kind == ExecutorKind::Maa && task.is_none() {
        bail!(
            "--executor maa 需要 --task <名称>（accounts/{account_key}/maa/tasks/ 下的任务文件）"
        );
    }

    // 一账号至多一个活跃会话（§6.5）
    if let Some(s) = store
        .active_session_by_account_key(&account_key)
        .map_err(|e| anyhow::anyhow!("{e}"))?
    {
        bail!("账号 {account_key} 已有活跃会话 #{}（{}）", s.id, s.state);
    }

    // 会话行 + 设备租约
    let session_id = store
        .create_session(arkreunion_core::store::NewSession {
            account_key: &account.key,
            device_name: &dev.name,
            executor: "mower",
            runner: Some("process"),
            state: "created",
            mower_port: None,
            slice_deadline_ms: None,
            max_runtime_deadline_ms: None,
        })
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let holder = format!("session:{session_id}");
    if let Err(e) = store.acquire_lease(&dev.name, &holder) {
        let _ = store.finish_session(session_id, "cancelled", "cancelled", None, "lease_conflict");
        bail!("{e}");
    }
    let mut released = LeaseOnDrop {
        store: store.clone(),
        device: dev.name.clone(),
        holder: holder.clone(),
        done: false,
    };

    let port = if kind == ExecutorKind::Mower {
        let p = store
            .alloc_port("mower_webview", &holder, cfg.ports.mower_session_range)
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        store
            .set_mower_port(session_id, p)
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        Some(p)
    } else {
        None
    };
    let now = arkreunion_core::store::now_ms();
    let slice_ms = match slice.as_deref() {
        Some(s) => arkreunion_core::config::HumanDuration::parse(s)
            .map_err(anyhow::Error::msg)?
            .0
            .as_millis() as i64,
        None => cfg.scheduler.default_slice.0.as_millis() as i64,
    };
    let max_runtime_ms = cfg.scheduler.max_session_runtime.0.as_millis() as i64;
    store
        .update_session_state(
            session_id,
            "queued",
            "queued",
            Some(&format!("手动会话：{executor_name}")),
        )
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    let ctx = SessionCtx {
        session_id,
        account: account.clone(),
        endpoints: arkreunion_core::device::DeviceEndpoints {
            host_adb: dev.connection.host_adb.clone(),
            docker_adb: dev.connection.docker_adb.clone(),
            docker_network: dev.connection.docker_network.clone(),
        },
        runner: arkreunion_core::model::RunnerKind::Process,
        mower_port: port,
        workdir: wd.root.clone(),
        mower_checkout: cfg.paths.mower_dir_expanded(),
        maa_task: task.clone(),
    };

    let executor = build_executor(kind);
    store
        .update_session_state(session_id, "running", "executor_starting", None)
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let handle = executor.start(&ctx).await.map_err(|e| {
        let _ = store.finish_session(
            session_id,
            "failed",
            "executor_crash",
            Some(&e.to_string()),
            "start_failed",
        );
        let _ = store.release_ports(&holder);
        anyhow::anyhow!("执行器启动失败：{e}")
    })?;
    store
        .set_executor_handle(
            session_id,
            &handle.locator,
            Some(now + slice_ms),
            Some(now + max_runtime_ms),
        )
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    store
        .update_session_state(
            session_id,
            "running",
            "executor_started",
            Some(&handle.locator),
        )
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    println!(
        "✓ 会话 #{} 已启动（{executor_name}，设备 {}）",
        session_id, dev.name
    );
    println!("  句柄：{}", handle.locator);
    if let Some(p) = port {
        println!(
            "  mower UI 深链：http://127.0.0.1:{p}/?token={}（配置编辑在此进行，改动持久化到 bundle）",
            ctx.webview_token()
        );
    }
    println!(
        "  日志：{}",
        wd.logs_dir()
            .join("sessions")
            .join(format!("{session_id}.log"))
            .display()
    );
    println!("  停止：arkreunion session stop {session_id}");

    released.done = true; // 会话存活期间租约保持
    Ok(())
}

async fn stop(
    wd: Workdir,
    _guard: WorkdirGuard,
    store: Arc<Store>,
    id: Option<u64>,
    account: Option<String>,
    grace: Option<String>,
) -> Result<()> {
    let session = match (id, account) {
        (Some(id), _) => store
            .get_session(id)
            .map_err(|e| anyhow::anyhow!("{e}"))?
            .ok_or_else(|| anyhow::anyhow!("会话 {id} 不存在"))?,
        (None, Some(acc)) => store
            .active_session_by_account_key(&acc)
            .map_err(|e| anyhow::anyhow!("{e}"))?
            .ok_or_else(|| anyhow::anyhow!("账号 {acc} 无活跃会话"))?,
        (None, None) => bail!("请提供会话 id 或 --account"),
    };
    stop_session(&wd, &store, session, grace).await
}

async fn stop_session(
    wd: &Workdir,
    store: &Arc<Store>,
    s: SessionRow,
    grace: Option<String>,
) -> Result<()> {
    if matches!(
        s.state.as_str(),
        "finished" | "failed" | "cancelled" | "timed_out" | "switch_failed"
    ) {
        bail!("会话 #{} 已是终态（{}）", s.id, s.state);
    }
    let grace = match grace.as_deref() {
        Some(g) => Duration::from_millis(
            g.parse::<arkreunion_core::config::HumanDuration>()
                .map_err(anyhow::Error::msg)?
                .0
                .as_millis() as u64,
        ),
        None => MANUAL_STOP_GRACE,
    };
    let holder = format!("session:{}", s.id);
    println!("停止会话 #{}（{} @ {}）…", s.id, s.executor, s.device_name);
    let _ = store.update_session_state(s.id, "draining", "drain", Some("手动停止"));

    // 优雅：mower 走 POST /stop；maa 无停止 API 直接超时杀
    let mut stopped = false;
    if s.executor == "mower"
        && let Some(port) = s.mower_port
    {
        let token = format!("arkreunion-s{}", s.id);
        match stop_via_http(port, &token).await {
            Ok(()) => {
                println!("  已发送 POST /stop");
                stopped = true;
            }
            Err(e) => println!("  POST /stop 不可达（{e}），等待进程退出或超时强杀"),
        }
    }
    // 进程级兜底：locator 里的 pid
    let pid = s.locator.as_deref().and_then(|l| parse_locator(l).0);
    let deadline = tokio::time::Instant::now() + grace;
    while tokio::time::Instant::now() < deadline {
        if !pid_alive(pid) {
            stopped = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    if !stopped && let Some(pid) = pid {
        kill_pid(pid);
        println!("  优雅停止超时，已强杀 pid={pid}");
    }

    let _ = store.release_ports(&holder);
    let _ = store.finish_session(s.id, "finished", "cancelled", None, "stopped_manually");
    let _ = store.release_lease(&s.device_name, &holder);
    println!(
        "✓ 会话 #{} 已停止（日志：{}）",
        s.id,
        wd.logs_dir()
            .join("sessions")
            .join(format!("{}.log", s.id))
            .display()
    );
    Ok(())
}

async fn list(wd: &Workdir, limit: u32) -> Result<()> {
    let store = Store::open(&wd.db_path()).map_err(|e| anyhow::anyhow!("{e}"))?;
    let rows = store
        .list_sessions(limit)
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    if rows.is_empty() {
        println!("（无会话记录）");
        return Ok(());
    }
    for s in rows {
        println!(
            "#{} {} {} {} {} port={:?} 起于 {:?} 止于 {:?} outcome={:?}",
            s.id,
            s.state,
            s.executor,
            s.account_key,
            s.device_name,
            s.mower_port,
            s.started_at_ms,
            s.ended_at_ms,
            s.outcome
        );
    }
    Ok(())
}

async fn logs(wd: &Workdir, id: u64, follow: bool) -> Result<()> {
    let path = wd.logs_dir().join("sessions").join(format!("{id}.log"));
    if !path.exists() {
        bail!("会话日志不存在：{}", path.display());
    }
    use std::io::Read;
    let mut f = std::fs::File::open(&path).map_err(|e| anyhow::anyhow!("{e}"))?;
    let mut pos: u64 = 0;
    loop {
        let len = f.metadata().map_err(|e| anyhow::anyhow!("{e}"))?.len();
        if len > pos {
            f.seek(std::io::SeekFrom::Start(pos))
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            let mut buf = String::new();
            let n = f
                .read_to_string(&mut buf)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            print!("{buf}");
            pos += n as u64;
        }
        if !follow {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

fn pid_alive(pid: Option<u32>) -> bool {
    match pid {
        None => false,
        Some(p) => {
            #[cfg(unix)]
            {
                std::path::Path::new(&format!("/proc/{p}")).exists()
            }
            #[cfg(windows)]
            {
                std::process::Command::new("tasklist")
                    .args(["/FI", &format!("PID eq {p}")])
                    .output()
                    .map(|o| String::from_utf8_lossy(&o.stdout).contains(&p.to_string()))
                    .unwrap_or(false)
            }
        }
    }
}

fn kill_pid(pid: u32) {
    #[cfg(unix)]
    {
        let _ = std::process::Command::new("kill")
            .arg(pid.to_string())
            .status();
    }
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("taskkill")
            .args(["/F", "/PID", &pid.to_string()])
            .status();
    }
}

struct LeaseOnDrop {
    store: Arc<Store>,
    device: String,
    holder: String,
    done: bool,
}

impl Drop for LeaseOnDrop {
    fn drop(&mut self) {
        if !self.done {
            let _ = self.store.release_lease(&self.device, &self.holder);
        }
    }
}
