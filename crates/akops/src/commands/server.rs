//! `akops server`：daemon + 调度器宿主 + 最小控制 API（任务 8 的骨架）。
//!
//! daemon 持有工作目录单写者锁常驻（ADR-0001 D7）；Web 控制台/WS/OpenAPI
//! 属任务 8 后续，此处先交付调度器运行所必需的端点：
//! `GET /api/status`、`POST /api/schedule/pause|resume`、`GET /api/sessions`、
//! `POST /api/sessions/{id}/drain`。鉴权：`server.token` 非空时 Bearer / `?token=`。

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use axum::extract::{Path, Query, State};
use axum::http::{Request, StatusCode};
use axum::middleware::{self, Next};
use axum::response::IntoResponse;
use axum::{Json, Router};
use serde_json::json;

use akops_core::config::{AkopsConfig, Workdir};
use akops_core::executor::{MaaCliExecutor, MowerProcessExecutor};
use akops_core::lock::WorkdirGuard;
use akops_core::model::ExecutorKind;
use akops_core::scheduler::{EngineDeps, EngineHandle, ExecutorFactory, run_engine};
use akops_core::store::Store;

pub(crate) async fn run(
    wd: Workdir,
    _guard: WorkdirGuard,
    port: Option<u16>,
    bind: Option<String>,
    open: bool,
) -> Result<()> {
    let mut cfg: AkopsConfig = wd.load_config()?;
    if let Some(p) = port {
        cfg.server.port = p;
    }
    if let Some(b) = bind {
        cfg.server.bind = b;
    }
    cfg.validate().map_err(anyhow::Error::from)?;
    let store = crate::commands::open_store(&wd)?;
    crate::commands::recover_orphans(&store);

    let device = crate::commands::resolve_device(&wd, None)?;
    let factory: ExecutorFactory = Arc::new(|kind| match kind {
        ExecutorKind::Mower => Ok(Box::new(MowerProcessExecutor::new(if cfg!(windows) {
            "python"
        } else {
            "python3"
        }))),
        ExecutorKind::Maa => Ok(Box::new(MaaCliExecutor::new("maa"))),
    });

    let deps = EngineDeps {
        wd: wd.clone(),
        cfg: cfg.clone(),
        store: store.clone(),
        device,
        factory,
        tick: Duration::from_secs(15),
        switch_timeout: Duration::from_secs(5 * 60),
        maa_bin: "maa".into(),
        backoff_override: None,
    };
    let (stop_rx, handle) = EngineHandle::new();
    let engine_task = tokio::spawn(run_engine(deps, handle.clone(), stop_rx));

    // daemon.json（status/CLI 探测用）
    let port = cfg.server.port;
    let daemon_info = json!({
        "pid": std::process::id(),
        "port": port,
        "started_at": chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%:z").to_string(),
    });
    std::fs::write(
        wd.daemon_json(),
        serde_json::to_string_pretty(&daemon_info).unwrap(),
    )
    .map_err(|e| anyhow::anyhow!("写 daemon.json 失败：{e}"))?;

    let state = AppState {
        wd: wd.clone(),
        store: store.clone(),
        handle: handle.clone(),
        token: cfg.server.token.clone(),
    };
    let app = router(state);
    let addr: SocketAddr = format!(
        "{}:{}",
        if cfg.server.bind == "localhost" {
            "127.0.0.1"
        } else {
            &cfg.server.bind
        },
        port
    )
    .parse()
    .context("解析监听地址失败")?;
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("绑定 {addr} 失败"))?;
    let url = format!("http://{addr}");
    println!("✓ akops daemon 已启动：{url}（Web 控制台属任务 8；Ctrl-C 停止）");
    println!(
        "  调度器：单设备 {}，15s 轮询；schedule pause/resume 经 API 生效",
        deps_device_name(&wd)
    );
    if open {
        let _ = open_browser(&url);
    }

    // Ctrl-C → 停引擎（优雅 drain）→ 摘除 daemon.json
    let wd_for_exit = wd.clone();
    let serve = axum::serve(listener, app).with_graceful_shutdown({
        async move {
            shutdown_signal().await;
            println!("\n收到关停信号（SIGINT/SIGTERM）：正在优雅 drain 活跃会话…");
            let _ = handle.stop_tx.send(true);
            let _ = tokio::time::timeout(Duration::from_secs(40), engine_task).await;
            let _ = std::fs::remove_file(wd_for_exit.daemon_json());
        }
    });
    serve
        .await
        .map_err(|e| anyhow::anyhow!("HTTP 服务异常退出：{e}"))?;
    Ok(())
}

fn deps_device_name(wd: &Workdir) -> String {
    wd.device_names().join(", ")
}

fn open_browser(url: &str) -> Result<()> {
    #[cfg(unix)]
    let prog = "xdg-open";
    #[cfg(windows)]
    let prog = "cmd";
    #[cfg(windows)]
    {
        std::process::Command::new(prog)
            .args(["/C", "start", url])
            .spawn()?;
        return Ok(());
    }
    #[cfg(unix)]
    {
        std::process::Command::new(prog).arg(url).spawn()?;
        Ok(())
    }
}

// ---------- HTTP 层 ----------

#[derive(Clone)]
struct AppState {
    wd: Workdir,
    store: Arc<Store>,
    handle: EngineHandle,
    token: String,
}

fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/status", axum::routing::get(api_status))
        .route("/api/schedule/pause", axum::routing::post(api_pause))
        .route("/api/schedule/resume", axum::routing::post(api_resume))
        .route("/api/sessions", axum::routing::get(api_sessions))
        .route("/api/sessions/{id}/drain", axum::routing::post(api_drain))
        .layer(middleware::from_fn_with_state(state.clone(), auth_mw))
        .with_state(state)
}

/// 鉴权中间件：token 为空直接放行（默认仅 loopback）；否则校验 Bearer / ?token=。
async fn auth_mw(
    State(s): State<AppState>,
    req: Request<axum::body::Body>,
    next: Next,
) -> impl IntoResponse {
    if s.token.is_empty() {
        return next.run(req).await;
    }
    let supplied = req
        .headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(String::from)
        .or_else(|| {
            req.uri().query().and_then(|q| {
                q.split('&')
                    .find_map(|kv| kv.strip_prefix("token="))
                    .map(String::from)
            })
        });
    if supplied.as_deref() == Some(s.token.as_str()) {
        next.run(req).await
    } else {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "token 无效或缺失"})),
        )
            .into_response()
    }
}

async fn api_status(State(s): State<AppState>) -> impl IntoResponse {
    let accounts: Vec<_> =
        s.wd.load_all_accounts()
            .unwrap_or_default()
            .iter()
            .map(|a| {
                json!({
                    "id": a.id, "display_name": a.display_name, "server": a.server.to_string(),
                    "enabled": a.enabled, "priority": a.schedule.priority,
                    "windows": a.schedule.windows.iter().map(|w| json!({
                        "start": w.start, "end": w.end,
                        "executor": match w.executor {
                            akops_core::model::ScheduledExecutor::Mower => "mower",
                            akops_core::model::ScheduledExecutor::Maa => "maa",
                        },
                        "task": w.task,
                    })).collect::<Vec<_>>(),
                })
            })
            .collect();
    let devices: Vec<_> =
        s.wd.load_all_devices()
            .unwrap_or_default()
            .iter()
            .map(|d| json!({"name": d.name, "host_adb": d.connection.host_adb}))
            .collect();
    let active = s
        .handle
        .active
        .0
        .lock()
        .expect("active 锁 poisoned")
        .as_ref()
        .map(|a| a.session_id);
    Json(json!({
        "paused": s.handle.is_paused(),
        "active_session": active,
        "accounts": accounts,
        "devices": devices,
    }))
}

async fn api_pause(State(s): State<AppState>) -> impl IntoResponse {
    s.handle
        .pause
        .store(true, std::sync::atomic::Ordering::SeqCst);
    Json(json!({"paused": true}))
}

async fn api_resume(State(s): State<AppState>) -> impl IntoResponse {
    s.handle
        .pause
        .store(false, std::sync::atomic::Ordering::SeqCst);
    Json(json!({"paused": false}))
}

#[derive(serde::Deserialize)]
struct SessionsQuery {
    limit: Option<u32>,
}

async fn api_sessions(
    State(s): State<AppState>,
    Query(q): Query<SessionsQuery>,
) -> impl IntoResponse {
    let rows = s
        .store
        .list_sessions(q.limit.unwrap_or(20))
        .unwrap_or_default();
    Json(json!(
        rows.iter()
            .map(|r| json!({
                "id": r.id, "account_id": r.account_id, "device_name": r.device_name,
                "executor": r.executor, "state": r.state, "mower_port": r.mower_port,
                "started_at_ms": r.started_at_ms, "ended_at_ms": r.ended_at_ms,
                "outcome": r.outcome, "error": r.error,
            }))
            .collect::<Vec<_>>()
    ))
}

async fn api_drain(State(s): State<AppState>, Path(id): Path<u64>) -> impl IntoResponse {
    if s.handle
        .active
        .request_stop(id, akops_core::scheduler::StopReason::Manual)
    {
        Json(json!({"draining": id})).into_response()
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(json!({"error": format!("会话 {id} 非当前活跃会话")})),
        )
            .into_response()
    }
}

/// 关停信号：SIGINT（Ctrl-C）与 SIGTERM（kill，unix）统一处理。
async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut term = signal(SignalKind::terminate()).expect("注册 SIGTERM 失败");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = term.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

/// 供 schedule CLI 的轻量 HTTP 客户端（无 reqwest 依赖）：返回响应体文本。
pub(crate) async fn http_call(
    port: u16,
    method: &str,
    path_and_query: &str,
    token: &str,
) -> Result<String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .context("连接 daemon 失败")?;
    let sep = if path_and_query.contains('?') {
        '&'
    } else {
        '?'
    };
    let auth = if token.is_empty() {
        String::new()
    } else {
        format!("Authorization: Bearer {token}\r\n")
    };
    let req = format!(
        "{method} {path_and_query}{sep}token={token} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n{auth}Connection: close\r\nContent-Length: 0\r\n\r\n"
    );
    stream.write_all(req.as_bytes()).await?;
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).await?;
    let text = String::from_utf8_lossy(&buf);
    let body = text
        .split_once("\r\n\r\n")
        .map(|(_, b)| b.to_string())
        .unwrap_or_else(|| text.to_string());
    if !text.starts_with("HTTP/1.1 2") && !text.starts_with("HTTP/1.0 2") {
        bail!("daemon 返回非 2xx：{}", text.lines().next().unwrap_or("?"));
    }
    Ok(body)
}

/// 读 daemon.json 的端口（daemon 探测判据）。
pub(crate) fn daemon_port(wd: &Workdir) -> Option<u16> {
    let raw = std::fs::read_to_string(wd.daemon_json()).ok()?;
    let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
    v.get("port").and_then(|p| p.as_u64()).map(|p| p as u16)
}
