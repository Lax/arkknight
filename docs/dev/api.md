# API 参考

控制台与 API 的权威路由表在代码里：`crates/arkknight/src/commands/server.rs` 的
`router()` 函数。本文说明**实际已实现**的路由与约定（设计文档 §13.2 是含未实现项的草案）。

## 启动

```bash
arkknight server [--port N] [--bind ADDR] [--open]
```

daemon 持有工作目录单写者 flock，REST API 与 WebSocket 同端口。

## 鉴权

`server.token` **非空时**启用（默认 `127.0.0.1` + 空 token = 不鉴权）：

```
Authorization: Bearer <token>
```
或
```
?token=<token>
```

**只作用于 `/api/*`。** 静态资源（`/assets/*`）不经鉴权 —— 浏览器
`<script src>` 不会带 `?token=`，若一并拦截则控制台永久白屏。

401 响应体：

```json
{
  "error": "token 无效或缺失",
  "detail": "请求未携带 token",
  "hint": "token 来自工作目录配置 <path>/arkknight.toml[server].token。修改后需重启 daemon；控制台侧可在左侧「API token」框填写，或用 ?token=<值> 访问 http://127.0.0.1:7100/?token=<值>",
  "config_path": "<path>/arkknight.toml"
}
```

`detail` 区分「未携带」与「已提供但不一致」。

## 错误约定

**所有错误响应带 `hint`**，说明「去哪里改」：

```json
{ "error": "<发生了什么>", "hint": "<怎么修>" }
```

`hint` 按错误类型分流：配置项路径、缺失实体的已注册清单、建议执行的 CLI 命令。

例：账号不存在时 hint 会列出当前已注册的 key，并给出 `account add` 命令模板。

## 已实现路由

### 状态与调度

| 方法 | 路径 | 说明 |
|---|---|---|
| GET | `/api/status` | 总览：`{accounts, active_session, devices, paused}` |
| POST | `/api/schedule/pause` | 暂停自动调度（运行中会话不受影响） |
| POST | `/api/schedule/resume` | 恢复自动调度 |

### 会话

| 方法 | 路径 | 说明 |
|---|---|---|
| GET | `/api/sessions?limit=N` | 列出最近会话（默认 20） |
| POST | `/api/sessions` | 手动开会话，body `{account_key, executor, slice?, task?}` |
| POST | `/api/sessions/{id}/drain` | 优雅停止（超时后 pid 兜底强杀） |
| GET | `/api/sessions/{id}/logs?tail=N` | 读取会话日志尾部 |

`POST /api/sessions` 的失败语义：

- 已有活跃会话 → `409`，hint 给出 drain 命令
- executor 非 `mower|maa` → `400`
- `executor=maa` 未给 `task` → `400`
- 账号不存在 → `404`，hint 列出已注册 key

### 账号

| 方法 | 路径 | 说明 |
|---|---|---|
| GET | `/api/accounts` | 列出账号 |
| POST | `/api/accounts` | 创建，body `{key, server, account_name, uid?, display_name?, priority?, windows?}` |
| PATCH | `/api/accounts/{key}` | 改 `{enabled?, priority?, account_name?, display_name?, uid?}` |
| DELETE | `/api/accounts/{key}?confirm=1` | 删除（含 bundle，不可恢复，**须带 confirm**） |

- `key` 缺省校验：slug（小写字母/数字开头，含 `-`/`_`）
- `uid` 须为纯数字
- `account_name` 跨账号唯一性在创建时校验
- `uid` 与其它字段的校验失败按类型分流 hint（uid 格式 / account_name 重复 / key 非法）

账号响应体：

```json
{
  "key": "main", "display_name": "主号", "server": "official",
  "account_name": "123****8901", "uid": "1000123456",
  "enabled": true, "priority": 50, "slice": null,
  "windows": [{ "start": "08:00", "end": "12:00", "executor": "mower", "task": null }],
  "provisioned_on": ["redroid-main"]
}
```

### 设备

| 方法 | 路径 | 说明 |
|---|---|---|
| GET | `/api/devices` | 列出设备 |
| POST | `/api/devices/{name}/test` | adb 连通性 + 游戏包检测 |
| GET | `/api/devices/{name}/screenshot` | 设备截图，**返回 PNG 二进制**（非 JSON） |

`test` 响应：`{name, reachable, detail, game_packages}`。

`screenshot` 响应头 `Content-Type: image/png` + `Cache-Control: no-store`。
失败时 `503` + JSON hint（设备离线/息屏）。

实现要点（改这块前必读）：

- 用 `adb exec-out screencap -p`，**不能**用 `shell` —— 后者把 CRLF 转换进二进制流破坏 PNG
- 前置 `adb connect`：daemon 重启后 adb server 丢失连接记录，首次必失败
- 校验 PNG magic（`\x89PNG\r\n\x1a\n`）：息屏时 adb 返回空或文本
- 纯 adb 只读操作，不属 INV-1 禁止的自研游戏内自动化

### 维护

| 方法 | 路径 | 说明 |
|---|---|---|
| GET | `/api/doctor` | 环境体检报告 `{checks: [{id, level, detail, hint}]}` |

### WebSocket

| 路径 | 说明 |
|---|---|
| `ws://<host>/api/ws?events=1&token=<值>` | 事件流（会话状态机迁移、切号进度） |
| `ws://<host>/api/ws?logs=<session_id>&token=<值>` | 指定会话日志 tail（从文件尾 ~8KB 起，400ms 轮询增量） |

两个参数都不给会收到一条文本提示而非关闭连接。

事件流基于 `EventBus`（`tokio::sync::broadcast`）。订阅过慢会收到 `Lagged`
并跳过该批次 —— 事件流是**尽力而为**，权威记录在 SQLite `session_events`。

## 未实现（设计文档 §13.2 草案中的）

以下在草案里但**当前不存在**，不要照着草案集成：

- `GET /api/accounts/{key}`（只有 PATCH/DELETE）
- `POST /api/accounts/{key}/provision`
- `POST /api/switch`
- `DELETE /api/sessions/{id}`
- `GET /api/schedule`
- `POST /api/updates/*`、`GET /api/logs`、`GET /api/stats`、`/api/export`、`/api/import`
- `POST /api/devices/{name}/watermark`（M2）
- `/docs` Swagger UI（utoipa 未接入）

需走 daemon API 的能力目前只有 CLI（`arkknight session start` 等）。

## 契约测试

`server.rs` 内的 `mod tests` 用 `tower::ServiceExt::oneshot` 直接打 Router：

```rust
let res = router(state_with_token("secret"))
    .oneshot(HttpRequest::get("/assets/index.js").body(Body::empty()).unwrap())
    .await.unwrap();
assert_ne!(res.status(), StatusCode::UNAUTHORIZED);
```

`tower` 是 **dev-dependencies**，不进发布二进制。

现有五个测试：

| 测试 | 断言 |
|---|---|
| `静态资源不经鉴权_控制台可加载` | token 非空时 `/assets/*` **不得** 401（白屏回归防线） |
| `api_无token被拒_带token放行` | API 仍严格校验，`?token=` 与 `Bearer` 两条路都通 |
| `鉴权失败报错带hint与配置路径` | 401 带 `detail`/`hint`/`config_path`，hint 指向 `[server].token` |
| `不存在实体的报错列出已注册项` | 404 的 hint 含 `account add` 命令与已注册清单 |
| `创建账号可带uid` | uid 落盘、展示名回落 key、非数字被拒 |

**新增或修改路由时同步补测试** —— 现有测试即鉴权回归的防线。

## 静态资源

Web 控制台经 `rust-embed` 在**编译期**嵌入 `ui/dist`：

- release 构建：缺 `ui/dist` 会编译失败（特性：保证发布物带最新控制台）
- debug 构建：直接读盘，改完 `npm run build` 刷新即可
- 未构建时回退到指引页（提示跑 `npm install && npm run build`）

路径解析：非 `.` 结尾且非已知文件 → SPA 路由回退 `index.html`。