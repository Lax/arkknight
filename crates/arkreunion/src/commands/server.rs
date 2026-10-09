//! `arkreunion server`：daemon + 调度器宿主 + 控制 API + Web 控制台（任务 8）。
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

use arkreunion_core::config::{AkopsConfig, Workdir};
use arkreunion_core::device::build_backend;
use arkreunion_core::executor::{MaaCliExecutor, MowerProcessExecutor};
use arkreunion_core::lock::WorkdirGuard;
use arkreunion_core::model::{ExecutorKind, ScheduledExecutor};
use arkreunion_core::scheduler::{
    EngineDeps, EngineHandle, EngineShared, ExecutorFactory, run_engine, session::run_session_flow,
};
use arkreunion_core::store::Store;

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
    println!("✓ arkreunion daemon 已启动：{url}（控制台 {url}；Ctrl-C 停止）");
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
        .route(
            "/api/devices/{name}/screenshot",
            axum::routing::get(api_device_screenshot),
        )
        .route("/api/doctor", axum::routing::get(api_doctor))
        .route("/api/ws", axum::routing::get(api_ws))
        .fallback(ui_fallback)
        .layer(middleware::from_fn_with_state(state.clone(), auth_mw))
        .with_state(state)
}

/// 鉴权中间件：token 为空直接放行（默认仅 loopback）；否则校验 Bearer / ?token=。
///
/// 仅作用于 `/api/*`：控制台的 `<script src>`/`fetch` 不会自动带 `?token=`，
/// 若连静态资源一并拦截则控制台永远白屏（首页 200、assets 401）。
/// 静态资源本身不含敏感数据，且控制台仅在 loopback/受控网络暴露。
async fn auth_mw(
    State(s): State<AppState>,
    req: Request<axum::body::Body>,
    next: Next,
) -> impl IntoResponse {
    if s.token.is_empty() || !req.uri().path().starts_with("/api/") {
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
        let detail = if supplied.is_some() {
            "token 已提供但与 server.token 不一致"
        } else {
            "请求未携带 token"
        };
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "error": "token 无效或缺失",
                "detail": detail,
                "hint": format!(
                    "token 来自工作目录配置 {}[server].token。修改后需重启 daemon；\
                     控制台侧可在左侧「API token」框填写，或用 ?token=<值> 访问 \
                     http://{addr}/?token=<值>（前端会存入 localStorage）。",
                    s.wd.config_path().display(),
                    addr = cfg_addr_hint(&s.wd),
                ),
                "config_path": s.wd.config_path().display().to_string(),
            })),
        )
            .into_response()
    }
}

/// 列表为空时给出「（无）」，避免 hint 出现空字符串。
fn list_or_none(items: &[String]) -> String {
    if items.is_empty() {
        "（无）".to_string()
    } else {
        items.join("、")
    }
}

/// 报错里的控制台地址提示（读配置端口，失败则省略）。
fn cfg_addr_hint(wd: &Workdir) -> String {
    match wd.load_config() {
        Ok(cfg) => format!("127.0.0.1:{}", cfg.server.port),
        Err(_) => "127.0.0.1:7100".to_string(),
    }
}

/// 统一的「可操作」错误响应：`error` 面向人，`hint` 给出下一步命令/文件位置。
///
/// 控制台把 `hint` 直接展示，避免用户只看到干巴巴的「不存在」而无从下手。
fn actionable(err: impl std::fmt::Display, hint: impl Into<String>) -> axum::response::Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({"error": err.to_string(), "hint": hint.into()})),
    )
        .into_response()
}

async fn api_status(State(s): State<AppState>) -> impl IntoResponse {
    let accounts: Vec<_> =
        s.wd.load_all_accounts()
            .unwrap_or_default()
            .iter()
            .map(|a| {
                json!({
                    "key": a.key, "display_name": a.display_name, "server": a.server.to_string(),
                    "enabled": a.enabled, "priority": a.schedule.priority,
                    "windows": a.schedule.windows.iter().map(|w| json!({
                        "start": w.start, "end": w.end,
                        "executor": match w.executor {
                            arkreunion_core::model::ScheduledExecutor::Mower => "mower",
                            arkreunion_core::model::ScheduledExecutor::Maa => "maa",
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
                "id": r.id, "account_key": r.account_key, "device_name": r.device_name,
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
        .request_stop(id, arkreunion_core::scheduler::StopReason::Manual)
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

fn account_json(a: &arkreunion_core::model::Account) -> serde_json::Value {
    json!({
        "key": a.key, "display_name": a.display_name, "server": a.server.to_string(),
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
    /// 账号 key（本地定位键 = 目录名）
    key: String,
    #[serde(default)]
    display_name: Option<String>,
    server: String,
    account_name: String,
    /// 游戏 UID（纯数字）：配置后切号成功即 OCR 核验身份，防登错号串数据（§9.4）
    #[serde(default)]
    uid: Option<String>,
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
    fn into_model(self) -> anyhow::Result<arkreunion_core::model::TimeWindow> {
        let executor = match self.executor.as_str() {
            "mower" => ScheduledExecutor::Mower,
            "maa" => ScheduledExecutor::Maa,
            other => bail!("executor {other:?} 不合法：mower|maa"),
        };
        Ok(arkreunion_core::model::TimeWindow {
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
        Err(e) => {
            let msg = e.to_string();
            // 校验失败按错误类型给不同引导：uid 格式 / account_name 重复 / id 非法
            let hint = if msg.contains("uid") {
                "uid 须为纯数字游戏 UID（游戏内「个人名片」页可见）；留空则切号后跳过身份核验，doctor 会告警串数据风险".to_string()
            } else if msg.contains("account_name") {
                "切号匹配串须在该设备已登录账号中唯一：官服用打码手机号片段（如 123****8901），B服用昵称；最终以 MAA 运行结果为准".to_string()
            } else if msg.contains("id") {
                "id 决定目录 accounts/<id>：须小写字母/数字开头，仅含小写字母、数字、-、_"
                    .to_string()
            } else {
                "检查账号配置字段；`arkreunion doctor` 可体检".to_string()
            };
            (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": msg, "hint": hint})),
            )
                .into_response()
        }
    }
}

fn account_create_inner(
    wd: &Workdir,
    body: AccountCreateBody,
) -> anyhow::Result<arkreunion_core::model::Account> {
    let server: arkreunion_core::model::Server = body.server.parse().map_err(anyhow::Error::msg)?;
    let windows = body
        .windows
        .into_iter()
        .map(WindowBody::into_model)
        .collect::<anyhow::Result<Vec<_>>>()?;
    let acc = arkreunion_core::model::Account {
        key: body.key.clone(),
        display_name: body.display_name.unwrap_or_else(|| body.key.clone()),
        server,
        account_name: body.account_name,
        uid: body.uid,
        enabled: true,
        schedule: arkreunion_core::model::AccountSchedule {
            windows,
            priority: body.priority.unwrap_or(50),
            slice: None,
        },
        provisioned_on: vec![],
    };
    acc.validate().map_err(anyhow::Error::from)?;
    if wd.account_file(&acc.key).exists() {
        bail!("账号 {} 已存在", acc.key);
    }
    wd.save_account(&acc).map_err(anyhow::Error::from)?;
    let dups = wd.account_name_duplicates().map_err(anyhow::Error::from)?;
    if let Some((name, keys)) = dups.iter().find(|(_, keys)| keys.contains(&acc.key)) {
        let _ = wd.remove_account(&acc.key);
        bail!(
            "account_name {name:?} 已被 {} 使用（切号匹配串须唯一）",
            keys.join(",")
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
    Path(key): Path<String>,
    Json(body): Json<AccountPatchBody>,
) -> impl IntoResponse {
    let mut acc = match s.wd.load_account(&key) {
        Ok(a) => a,
        Err(e) => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({
                    "error": e.to_string(),
                    "hint": format!(
                        "账号不存在。已注册：{}；新增用 `arkreunion account add <key> --server official --account-name '<匹配串>'`（MAA 切号匹配串须唯一）",
                        list_or_none(&s.wd.account_keys())
                    ),
                })),
            )
                .into_response();
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
    Path(key): Path<String>,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    if q.get("confirm").map(|v| v.as_str()) != Some("1") {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "删除账号须带 ?confirm=1（连同 maa/mower bundle）"})),
        )
            .into_response();
    }
    match s.wd.remove_account(&key) {
        Ok(()) => Json(json!({"deleted": key})).into_response(),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": e.to_string(),
                "hint": format!(
                    "账号不存在。已注册：{}",
                    list_or_none(&s.wd.account_keys())
                ),
            })),
        )
            .into_response(),
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
            return (
                StatusCode::NOT_FOUND,
                Json(json!({
                    "error": e.to_string(),
                    "hint": format!(
                        "设备不存在。已注册：{}；新增用 `arkreunion device add <name> --host-adb <宿主adb地址> [--docker-adb <容器网络地址>]`",
                        list_or_none(&s.wd.device_names())
                    ),
                })),
            )
                .into_response();
        }
    };
    let backend = match build_backend(&dev, &cfg.paths.adb_path_expanded()) {
        Ok(b) => b,
        Err(e) => {
            return actionable(
                e,
                format!(
                    "设备后端不可用。检查 {}[paths].adb_path 指向的 adb 是否可用（容器内用 `arkreunion doctor` 体检）",
                    s.wd.config_path().display()
                ),
            );
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

/// 设备截屏（PNG）。前端以 `<img src=...>` 消费，故直接回二进制并带 no-store。
async fn api_device_screenshot(
    State(s): State<AppState>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    let cfg = match s.wd.load_config() {
        Ok(c) => c,
        Err(e) => return actionable(e, "读取工作目录配置失败，跑 `arkreunion doctor`"),
    };
    let dev = match s.wd.load_device(&name) {
        Ok(d) => d,
        Err(e) => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({
                    "error": e.to_string(),
                    "hint": format!(
                        "设备不存在。已注册：{}",
                        list_or_none(&s.wd.device_names())
                    ),
                })),
            )
                .into_response();
        }
    };
    let backend = match build_backend(&dev, &cfg.paths.adb_path_expanded()) {
        Ok(b) => b,
        Err(e) => return actionable(e, "设备后端不可用，跑 `arkreunion doctor`"),
    };
    match backend.screenshot().await {
        Ok(png) => (
            StatusCode::OK,
            [
                (
                    axum::http::header::CONTENT_TYPE,
                    "image/png".to_string(),
                ),
                (
                    axum::http::header::CACHE_CONTROL,
                    "no-store".to_string(),
                ),
            ],
            png,
        )
            .into_response(),
        Err(e) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "error": e.to_string(),
                "hint": "截屏失败多为设备离线/息屏：先 `arkreunion device test <name>`，再确认游戏或桌面已点亮屏幕",
            })),
        )
            .into_response(),
    }
}

async fn api_doctor(State(s): State<AppState>) -> impl IntoResponse {
    let report = arkreunion_core::doctor::run(&s.wd, &Default::default()).await;
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
    let active_id = s
        .handle
        .active
        .0
        .lock()
        .expect("active 锁 poisoned")
        .as_ref()
        .map(|a| a.session_id);
    if let Some(active_id) = active_id {
        return (
            StatusCode::CONFLICT,
            Json(json!({
                "error": "当前有会话运行中（调度器/手动）；请先 drain",
                "hint": format!(
                    "在控制台「会话」页点 drain，或 `arkreunion session drain {}`。\
                     若确认无会话却报此错，多为 daemon 重启前的会话残留：\
                     `arkreunion status` 查看，必要时重启 daemon（启动时会自动清理残留租约）",
                    active_id
                ),
            })),
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
            return (
                StatusCode::NOT_FOUND,
                Json(json!({
                    "error": e.to_string(),
                    "hint": format!(
                        "账号不存在。已注册：{}",
                        list_or_none(&s.wd.account_keys())
                    ),
                })),
            )
                .into_response();
        }
    };
    let slice = match body.slice.as_deref() {
        Some(txt) => match arkreunion_core::config::HumanDuration::parse(txt) {
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

const UI_NOT_BUILT: &str = r#"<!doctype html><meta charset="utf-8"><title>arkreunion</title>
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

#[cfg(test)]
mod tests {
    use super::*;
    use arkreunion_core::model::{Device, DeviceBackendKind, DeviceConnection};
    use axum::body::Body;
    use axum::http::Request as HttpRequest;
    use tower::util::ServiceExt;

    fn state_with_token(token: &str) -> AppState {
        let store = Arc::new(Store::open_in_memory().expect("打开内存库"));
        let (_, handle) = EngineHandle::new();
        let factory: ExecutorFactory = Arc::new(|_| Err("测试不需要真实执行器".to_string()));
        AppState {
            wd: Workdir::new(std::env::temp_dir().join("arkreunion-auth-test")),
            shared: Arc::new(EngineShared {
                wd: Workdir::new(std::env::temp_dir().join("arkreunion-auth-test")),
                cfg: AkopsConfig::default(),
                store: store.clone(),
                device: Device {
                    name: "d1".into(),
                    backend: DeviceBackendKind::External,
                    connection: DeviceConnection {
                        host_adb: "127.0.0.1:5555".into(),
                        docker_adb: None,
                        docker_network: None,
                    },
                    notes: String::new(),
                },
                factory,
                switch_timeout: Duration::from_secs(1),
                maa_bin: "maa".into(),
                backoff_override: None,
            }),
            store,
            handle,
            token: token.to_string(),
        }
    }

    /// token 非空时，静态资源（控制台 JS/CSS）不得被鉴权拦截——
    /// 浏览器不会给 `<script src>` 带 `?token=`，拦截会导致控制台永久白屏。
    #[tokio::test]
    async fn 静态资源不经鉴权_控制台可加载() {
        let app = router(state_with_token("secret"));
        let res = app
            .oneshot(
                HttpRequest::get("/assets/index.js")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_ne!(
            res.status(),
            StatusCode::UNAUTHORIZED,
            "assets 不得被鉴权拦截"
        );
    }

    /// 401 报错须可操作：带 hint（去哪儿改 token）与 config_path（哪个文件）。
    #[tokio::test]
    async fn 鉴权失败报错带hint与配置路径() {
        let app = router(state_with_token("secret"));
        let res = app
            .oneshot(HttpRequest::get("/api/status").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
        let v: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(res.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        let hint = v["hint"].as_str().unwrap_or_default();
        assert!(hint.contains("[server].token"), "hint 应指向配置项：{hint}");
        assert!(
            v["config_path"]
                .as_str()
                .unwrap_or_default()
                .ends_with("arkreunion.toml"),
            "应给出配置文件路径"
        );
        assert_eq!(v["detail"].as_str(), Some("请求未携带 token"));
    }

    /// 创建账号：uid 应能随请求写入（否则控制台无法录入核验依据）。
    #[tokio::test]
    async fn 创建账号可带uid() {
        let tmp = std::env::temp_dir().join(format!("arkreunion-acct-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let wd = Workdir::init(&tmp, &AkopsConfig::default()).unwrap();
        let acc = account_create_inner(
            &wd,
            AccountCreateBody {
                key: "main".into(),
                display_name: None,
                server: "official".into(),
                account_name: "123****8901".into(),
                uid: Some("1000123456".into()),
                priority: None,
                windows: vec![],
            },
        )
        .expect("创建应成功");
        assert_eq!(acc.uid.as_deref(), Some("1000123456"));
        assert_eq!(acc.display_name, "main", "展示名缺省应回落为 key");

        // 非数字 uid 应被 validate 拒绝，并给出可操作提示
        let bad = account_create_inner(
            &wd,
            AccountCreateBody {
                key: "bad".into(),
                display_name: None,
                server: "official".into(),
                account_name: "x".into(),
                uid: Some("12ab".into()),
                priority: None,
                windows: vec![],
            },
        )
        .unwrap_err()
        .to_string();
        assert!(bad.contains("uid"), "非数字 uid 应报错：{bad}");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// 不存在的账号/设备：报错应列出当前已注册项，便于用户自查拼写。
    #[tokio::test]
    async fn 不存在实体的报错列出已注册项() {
        let app = router(state_with_token(""));
        // PATCH 而非 GET：GET /api/accounts/{id} 未定义，会落到 SPA 回退返回 405
        let res = app
            .oneshot(
                HttpRequest::patch("/api/accounts/nope")
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
        let v: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(res.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        let hint = v["hint"].as_str().unwrap_or_default();
        assert!(hint.contains("account add"), "hint 应给出新增命令：{hint}");
        assert!(hint.contains("（无）"), "空列表应显式说明：{hint}");
    }

    /// 反面：API 仍必须校验 token。
    #[tokio::test]
    async fn api_无token被拒_带token放行() {
        let app = router(state_with_token("secret"));
        let res = app
            .clone()
            .oneshot(HttpRequest::get("/api/status").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

        let res = app
            .clone()
            .oneshot(
                HttpRequest::get("/api/status?token=secret")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_ne!(res.status(), StatusCode::UNAUTHORIZED);

        let res = app
            .oneshot(
                HttpRequest::get("/api/status")
                    .header("authorization", "Bearer secret")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_ne!(res.status(), StatusCode::UNAUTHORIZED);
    }
}
