//! `akops server`：daemon + 调度器宿主 + 控制 API + Web 控制台（任务 8）。
//!
//! daemon 持有工作目录单写者锁常驻（ADR-0001 D7）。API 面（鉴权：`server.token`
//! 非空时 Bearer / `?token=`）：
//! - 状态/调度：`GET /api/status`、`POST /api/schedule/pause|resume`
//! - 会话：`GET|POST /api/sessions`、`POST /api/sessions/{id}/drain`、`GET /api/sessions/{id}/logs`
//! - 账号：`GET|POST /api/accounts`、`PATCH|DELETE /api/accounts/{id}`
//! - 设备：`GET /api/devices`、`POST /api/devices/{name}/test`
//! - 维护：`GET /api/doctor`
//! - 实时：`WS /api/ws?logs=<id>`（日志 tail）、`WS /api/ws?events=1`（事件流）
//!
//! Web 控制台经 rust-embed 嵌入（`ui/dist`）；OpenAPI/统计页属任务 8 尾批。

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::{Request, StatusCode};
use axum::middleware::{self, Next};
use axum::response::IntoResponse;
use axum::{Json, Router};
use serde_json::json;

use akops_core::config::{AkopsConfig, Workdir};
use akops_core::device::build_backend;
use akops_core::executor::{MaaCliExecutor, MowerProcessExecutor};
use akops_core::lock::WorkdirGuard;
use akops_core::model::{ExecutorKind, ScheduledExecutor};
use akops_core::scheduler::{
    EngineDeps, EngineHandle, EngineShared, ExecutorFactory, run_engine, session::run_session_flow,
};
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

    let shared = Arc::new(EngineShared {
        wd: wd.clone(),
        cfg: cfg.clone(),
        store: store.clone(),
        device,
        factory,
        switch_timeout: Duration::from_secs(5 * 60),
        maa_bin: "maa".into(),
        backoff_override: None,
    });
    let deps = EngineDeps {
        shared: shared.clone(),
        tick: Duration::from_secs(15),
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
        shared: shared.clone(),
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
    println!("✓ akops daemon 已启动：{url}（控制台 {url}；Ctrl-C 停止）");
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
    shared: Arc<EngineShared>,
    handle: EngineHandle,
    token: String,
}

fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/status", axum::routing::get(api_status))
        .route("/api/schedule/pause", axum::routing::post(api_pause))
        .route("/api/schedule/resume", axum::routing::post(api_resume))
        .route(
            "/api/sessions",
            axum::routing::get(api_sessions).post(api_session_start),
        )
        .route("/api/sessions/{id}/drain", axum::routing::post(api_drain))
        .route(
            "/api/sessions/{id}/logs",
            axum::routing::get(api_session_logs),
        )
        .route(
            "/api/accounts",
            axum::routing::get(api_accounts).post(api_account_create),
        )
        .route(
            "/api/accounts/{id}",
            axum::routing::patch(api_account_patch).delete(api_account_delete),
        )
        .route("/api/devices", axum::routing::get(api_devices))
        .route(
            "/api/devices/{name}/test",
            axum::routing::post(api_device_test),
        )
        .route("/api/doctor", axum::routing::get(api_doctor))
        .route("/api/ws", axum::routing::get(api_ws))
        .fallback(ui_fallback)
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

// ---------- 账号 API ----------

async fn api_accounts(State(s): State<AppState>) -> impl IntoResponse {
    let accounts = s.wd.load_all_accounts().unwrap_or_default();
    Json(json!(accounts.iter().map(account_json).collect::<Vec<_>>()))
}

fn account_json(a: &akops_core::model::Account) -> serde_json::Value {
    json!({
        "id": a.id, "display_name": a.display_name, "server": a.server.to_string(),
        "account_name": a.account_name, "enabled": a.enabled, "uid": a.uid,
        "priority": a.schedule.priority,
        "slice": a.schedule.slice.as_ref().map(|d| d.to_string()),
        "windows": a.schedule.windows.iter().map(|w| json!({
            "start": w.start, "end": w.end,
            "executor": match w.executor {
                ScheduledExecutor::Mower => "mower",
                ScheduledExecutor::Maa => "maa",
            },
            "task": w.task,
        })).collect::<Vec<_>>(),
        "provisioned_on": a.provisioned_on,
    })
}

#[derive(serde::Deserialize)]
struct AccountCreateBody {
    id: String,
    #[serde(default)]
    display_name: Option<String>,
    server: String,
    account_name: String,
    #[serde(default)]
    priority: Option<u8>,
    #[serde(default)]
    windows: Vec<WindowBody>,
}

#[derive(serde::Deserialize)]
struct WindowBody {
    start: String,
    end: String,
    executor: String,
    #[serde(default)]
    task: Option<String>,
}

impl WindowBody {
    fn into_model(self) -> anyhow::Result<akops_core::model::TimeWindow> {
        let executor = match self.executor.as_str() {
            "mower" => ScheduledExecutor::Mower,
            "maa" => ScheduledExecutor::Maa,
            other => bail!("executor {other:?} 不合法：mower|maa"),
        };
        Ok(akops_core::model::TimeWindow {
            start: self.start,
            end: self.end,
            executor,
            task: self.task,
        })
    }
}

async fn api_account_create(
    State(s): State<AppState>,
    Json(body): Json<AccountCreateBody>,
) -> impl IntoResponse {
    match account_create_inner(&s.wd, body) {
        Ok(a) => (StatusCode::CREATED, Json(account_json(&a))).into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

fn account_create_inner(
    wd: &Workdir,
    body: AccountCreateBody,
) -> anyhow::Result<akops_core::model::Account> {
    let server: akops_core::model::Server = body.server.parse().map_err(anyhow::Error::msg)?;
    let windows = body
        .windows
        .into_iter()
        .map(WindowBody::into_model)
        .collect::<anyhow::Result<Vec<_>>>()?;
    let acc = akops_core::model::Account {
        id: body.id.clone(),
        display_name: body.display_name.unwrap_or_else(|| body.id.clone()),
        server,
        account_name: body.account_name,
        uid: None,
        enabled: true,
        schedule: akops_core::model::AccountSchedule {
            windows,
            priority: body.priority.unwrap_or(50),
            slice: None,
        },
        provisioned_on: vec![],
    };
    acc.validate().map_err(anyhow::Error::from)?;
    if wd.account_file(&acc.id).exists() {
        bail!("账号 {} 已存在", acc.id);
    }
    wd.save_account(&acc).map_err(anyhow::Error::from)?;
    let dups = wd.account_name_duplicates().map_err(anyhow::Error::from)?;
    if let Some((name, ids)) = dups.iter().find(|(_, ids)| ids.contains(&acc.id)) {
        let _ = wd.remove_account(&acc.id);
        bail!(
            "account_name {name:?} 已被 {} 使用（切号匹配串须唯一）",
            ids.join(",")
        );
    }
    Ok(acc)
}

#[derive(serde::Deserialize)]
struct AccountPatchBody {
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    priority: Option<u8>,
    #[serde(default)]
    account_name: Option<String>,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    uid: Option<String>,
}

async fn api_account_patch(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<AccountPatchBody>,
) -> impl IntoResponse {
    let mut acc = match s.wd.load_account(&id) {
        Ok(a) => a,
        Err(e) => {
            return (StatusCode::NOT_FOUND, Json(json!({"error": e.to_string()}))).into_response();
        }
    };
    if let Some(v) = body.enabled {
        acc.enabled = v;
    }
    if let Some(v) = body.priority {
        acc.schedule.priority = v;
    }
    if let Some(v) = body.account_name {
        acc.account_name = v;
    }
    if let Some(v) = body.display_name {
        acc.display_name = v;
    }
    if let Some(v) = body.uid {
        acc.uid = if v.is_empty() { None } else { Some(v) };
    }
    if let Err(e) = acc.validate() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": e.to_string()})),
        )
            .into_response();
    }
    match s.wd.save_account(&acc) {
        Ok(()) => Json(account_json(&acc)).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

async fn api_account_delete(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    if q.get("confirm").map(|v| v.as_str()) != Some("1") {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "删除账号须带 ?confirm=1（连同 maa/mower bundle）"})),
        )
            .into_response();
    }
    match s.wd.remove_account(&id) {
        Ok(()) => Json(json!({"deleted": id})).into_response(),
        Err(e) => (StatusCode::NOT_FOUND, Json(json!({"error": e.to_string()}))).into_response(),
    }
}

// ---------- 设备 / 维护 ----------

async fn api_devices(State(s): State<AppState>) -> impl IntoResponse {
    let devices = s.wd.load_all_devices().unwrap_or_default();
    Json(json!(
        devices
            .iter()
            .map(|d| json!({
                "name": d.name, "backend": d.backend.to_string(),
                "host_adb": d.connection.host_adb,
                "docker_adb": d.connection.docker_adb,
                "docker_network": d.connection.docker_network,
                "docker_compatible": d.docker_compatible(),
                "notes": d.notes,
            }))
            .collect::<Vec<_>>()
    ))
}

async fn api_device_test(State(s): State<AppState>, Path(name): Path<String>) -> impl IntoResponse {
    let cfg = match s.wd.load_config() {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": e.to_string()})),
            )
                .into_response();
        }
    };
    let dev = match s.wd.load_device(&name) {
        Ok(d) => d,
        Err(e) => {
            return (StatusCode::NOT_FOUND, Json(json!({"error": e.to_string()}))).into_response();
        }
    };
    let backend = match build_backend(&dev, &cfg.paths.adb_path_expanded()) {
        Ok(b) => b,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": e.to_string()})),
            )
                .into_response();
        }
    };
    #[allow(unused_imports)]
    use axum::response::IntoResponse as _;
    let health = backend.health().await;
    let packages = backend.detect_game_packages().await.unwrap_or_default();
    let (reachable, detail) = match &health {
        Ok(h) => (h.reachable, h.detail.clone()),
        Err(e) => (false, e.to_string()),
    };
    Json(json!({
        "name": name,
        "reachable": reachable,
        "detail": detail,
        "game_packages": packages,
    }))
    .into_response()
}

async fn api_doctor(State(s): State<AppState>) -> impl IntoResponse {
    let report = akops_core::doctor::run(&s.wd, &Default::default()).await;
    Json(serde_json::to_value(&report).unwrap_or(json!({"checks": []})))
}

// ---------- 会话：手动发起 / 日志 ----------

#[derive(serde::Deserialize)]
struct SessionStartBody {
    account: String,
    #[serde(default = "default_executor")]
    executor: String,
    #[serde(default)]
    slice: Option<String>,
    #[serde(default)]
    task: Option<String>,
}

fn default_executor() -> String {
    "mower".into()
}

async fn api_session_start(
    State(s): State<AppState>,
    Json(body): Json<SessionStartBody>,
) -> impl IntoResponse {
    // 引擎活跃会话进行中则拒绝（M1 单设备串行；不抢占）
    if s.handle
        .active
        .0
        .lock()
        .expect("active 锁 poisoned")
        .is_some()
    {
        return (
            StatusCode::CONFLICT,
            Json(json!({"error": "当前有会话运行中（调度器/手动）；请先 drain"})),
        )
            .into_response();
    }
    let kind = match body.executor.as_str() {
        "mower" => ExecutorKind::Mower,
        "maa" => ExecutorKind::Maa,
        other => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": format!("executor {other:?} 不合法：mower|maa")})),
            )
                .into_response();
        }
    };
    let account = match s.wd.load_account(&body.account) {
        Ok(a) => a,
        Err(e) => {
            return (StatusCode::NOT_FOUND, Json(json!({"error": e.to_string()}))).into_response();
        }
    };
    let slice = match body.slice.as_deref() {
        Some(txt) => match akops_core::config::HumanDuration::parse(txt) {
            Ok(d) => Duration::from_millis(d.0.as_millis() as u64),
            Err(e) => return (StatusCode::BAD_REQUEST, Json(json!({"error": e}))).into_response(),
        },
        None => account
            .schedule
            .slice
            .map(|d| Duration::from_millis(d.0.as_millis() as u64))
            .unwrap_or(Duration::from_millis(
                s.shared.cfg.scheduler.default_slice.0.as_millis() as u64,
            )),
    };

    // 手动会话期间暂停自动调度，结束后还原（§10.3 插队不抢占）
    let prev_paused = s.handle.is_paused();
    s.handle
        .pause
        .store(true, std::sync::atomic::Ordering::SeqCst);

    let shared = s.shared.clone();
    let handle = s.handle.clone();
    tokio::spawn(async move {
        let _ = run_session_flow(
            &shared.wd,
            &shared.cfg,
            &shared.store,
            &shared.device,
            &account,
            kind,
            body.task,
            slice,
            &shared.factory,
            &handle.active,
            shared.switch_timeout,
            &shared.maa_bin,
            &handle.events,
        )
        .await;
        handle
            .pause
            .store(prev_paused, std::sync::atomic::Ordering::SeqCst);
    });
    (
        StatusCode::ACCEPTED,
        Json(json!({"started": true, "account": body.account})),
    )
        .into_response()
}

async fn api_session_logs(
    State(s): State<AppState>,
    Path(id): Path<u64>,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    let tail: usize = q.get("tail").and_then(|v| v.parse().ok()).unwrap_or(200);
    let path = s.wd.logs_dir().join("sessions").join(format!("{id}.log"));
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({"error": format!("日志读取失败：{e}")})),
            )
                .into_response();
        }
    };
    let lines: Vec<&str> = content.lines().collect();
    let start = lines.len().saturating_sub(tail);
    (
        [(
            axum::http::header::CONTENT_TYPE,
            "text/plain; charset=utf-8",
        )],
        lines[start..].join("\n"),
    )
        .into_response()
}

// ---------- WebSocket ----------

#[derive(serde::Deserialize)]
struct WsQuery {
    #[serde(default)]
    logs: Option<u64>,
    #[serde(default)]
    events: Option<String>,
}

async fn api_ws(
    State(s): State<AppState>,
    Query(q): Query<WsQuery>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| async move { handle_ws(s, q, socket).await })
}

async fn handle_ws(state: AppState, q: WsQuery, mut socket: WebSocket) {
    if let Some(id) = q.logs {
        ws_tail_logs(state, id, socket).await;
    } else if q.events.as_deref() == Some("1") {
        ws_events(state, socket).await;
    } else {
        let _ = send_text(&mut socket, "参数不合法：?logs=<session_id> 或 ?events=1").await;
    }
}

async fn send_text(socket: &mut WebSocket, text: &str) -> Result<(), axum::Error> {
    socket.send(Message::text(text.to_string())).await
}

/// 日志 tail：从文件尾部 ~8KB 起，轮询增量推送。
async fn ws_tail_logs(state: AppState, id: u64, mut socket: WebSocket) {
    let path = state
        .wd
        .logs_dir()
        .join("sessions")
        .join(format!("{id}.log"));
    if !path.exists() {
        let _ = send_text(&mut socket, "（无日志文件）").await;
        return;
    }
    let mut offset = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    offset = offset.saturating_sub(8192);
    let mut tick = tokio::time::interval(Duration::from_millis(400));
    loop {
        tokio::select! {
            msg = socket.recv() => {
                if msg.is_none() {
                    return; // 客户端断开
                }
            }
            _ = tick.tick() => {
                let Ok(meta) = tokio::fs::metadata(&path).await else { continue };
                let len = meta.len();
                if len > offset {
                    use tokio::io::{AsyncReadExt, AsyncSeekExt};
                    let Ok(mut f) = tokio::fs::File::open(&path).await else { continue };
                    if f.seek(std::io::SeekFrom::Start(offset)).await.is_err() { continue; }
                    let mut buf = String::new();
                    if f.read_to_string(&mut buf).await.is_ok() {
                        offset = len;
                        for line in buf.lines() {
                            if send_text(&mut socket, line).await.is_err() {
                                return;
                            }
                        }
                    }
                }
            }
        }
    }
}

/// 事件流：转发引擎 EventBus 广播。
async fn ws_events(state: AppState, mut socket: WebSocket) {
    let mut rx = state.handle.events.0.subscribe();
    loop {
        tokio::select! {
            msg = socket.recv() => {
                if msg.is_none() {
                    return;
                }
            }
            ev = rx.recv() => {
                match ev {
                    Ok(line) => {
                        if send_text(&mut socket, &line).await.is_err() {
                            return;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => return,
                }
            }
        }
    }
}

// ---------- Web 控制台（rust-embed） ----------

#[derive(rust_embed::RustEmbed)]
#[folder = "../../ui/dist/"]
struct UiAssets;

const UI_NOT_BUILT: &str = r#"<!doctype html><meta charset="utf-8"><title>akops</title>
<body style="font-family:system-ui;max-width:40em;margin:3em auto;line-height:1.6">
<h2>控制台前端未构建</h2>
<p>API 已可用（<code>/api/status</code> 等）。构建前端：</p>
<pre>cd ui && npm install && npm run build</pre>
<p>构建产物经 rust-embed 嵌入（debug 构建直接读盘，改完 npm run build 刷新即可）。</p>
"#;

async fn ui_fallback(uri: axum::http::Uri) -> impl IntoResponse {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    match UiAssets::get(path) {
        Some(asset) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            (
                [(axum::http::header::CONTENT_TYPE, mime.as_ref().to_string())],
                asset.data,
            )
                .into_response()
        }
        None => {
            if path == "index.html" || !path.contains('.') {
                // SPA 路由回退 index；index 缺失则给构建指引
                match UiAssets::get("index.html") {
                    Some(asset) => (
                        [(
                            axum::http::header::CONTENT_TYPE,
                            "text/html; charset=utf-8".to_string(),
                        )],
                        asset.data,
                    )
                        .into_response(),
                    None => (
                        [(
                            axum::http::header::CONTENT_TYPE,
                            "text/html; charset=utf-8".to_string(),
                        )],
                        UI_NOT_BUILT,
                    )
                        .into_response(),
                }
            } else {
                (StatusCode::NOT_FOUND, "not found").into_response()
            }
        }
    }
}
