# AGENTS.md — arkreunion AI 协作规范

> 本文档面向在 arkreunion 仓库工作的 AI 编程助手（以及新加入的人类开发者）。
> 人类开发者请同时阅读 `docs/dev/README.md`；最终用户文档在 `docs/user/`。

## 项目是什么

arkreunion 是一个跨平台（Linux/Windows/macOS）的明日方舟**多账号编排器**：编排 MAA（账号切换/任务）与 mower（基建排班）等执行器，在少量安卓设备（redroid/原生模拟器）上自动轮转运行多个游戏账号。**它自己不做任何游戏内图像识别与操作自动化。**

- 总体设计（必读）：[`docs/arkreunion-design.md`](../arkreunion-design.md)
- 架构决策与理由：[`adr/`](./adr/)（改动架构前先读相关 ADR；新决策先写 ADR）

## 命名约定（易错，先读）

`Account` 有四个标识字段，**语义各不相干，改动时不要混**：

| 字段 | 归属 | 含义 | 出现位置 |
|---|---|---|---|
| `key` | **本项目内部** | 本地定位键 | 目录 `accounts/<key>/`、CLI 位置参数、API body/路径 |
| `display_name` | 内部 | 展示名 | 控制台/日志 |
| `account_name` | 游戏侧 | MAA 切号匹配串 | 物化的 `tasks/startup.toml` |
| `uid` | 游戏侧 | 游戏 UID，切号后 OCR 核验 | UID 核验 pipeline |

`key` **不表达任何游戏身份**。曾用 `id`，2026-10-09 改名 `key`（BREAKING，
serde `alias = "id"` 兼容旧文件；SQLite 列名同步 v1→v2）。

**为什么不用 `slot`**：与 `scheduler::ActiveSlot`（当前活跃会话槽）撞名。
**为什么不用 `profile`**：MAA 的 `profiles/default.toml` 已占用该词。

## 代码地图（随实现更新）

```
crates/arkreunion-core/      库：领域模型/配置/物化/调度器/执行器/设备后端/持久化
  src/model/            Account, Device, Session, SchedulePolicy（见设计文档 §6）
  src/config/           arkreunion.toml、工作目录布局、环境探测
  src/materialize/      MAA TOML 与 mower conf.yml 的物化与白名单 patch（§11）
  src/device/           DeviceBackend trait + external/（+ redroid/ M2）
  src/executor/         Executor trait + maa/ + mower/（ProcessRunner；DockerRunner 未做）
  src/scheduler/        状态机、队列、看门狗、退避（§10）
  src/switch/           账号切换的唯一实现入口（§9，受 INV-1 约束）
  src/store/            SQLite(rusqlite, WAL) + schema v1→v2 迁移
  src/doctor/           环境体检
crates/arkreunion/           CLI(clap) + axum server + rust-embed 前端
  src/commands/         各子命令；server.rs 内含 Router + 鉴权 + WS + API 测试
ui/                     Vue3 + Vite + Naive UI 控制台源码
docs/                   本文档树
```

### 当前状态（2026-10-09）

**已实现**：

- workspace 骨架、域模型与配置层、环境探测 + doctor、DeviceBackend/Executor trait + External 后端
- 物化器（MAA/mower，golden 快照）、CLI `init/doctor/status/account/device/completions`
- **store 层**：rusqlite WAL + schema 迁移（v1→v2）+ 租约/端口记账/logins + 单写者 flock
- **MaaCliExecutor + `switch::execute_switch`**：INV-1 唯一路径（force-stop → 物化 →
  `maa run startup -p default --batch` → 重试退避 → switch_log）；maa 路径可注入，fake 测试覆盖
- **provision**：投屏指引 + 回车确认 + UID 录入 + logins 记录 + 租约互斥
- **MowerProcessExecutor + session start/stop/list/logs**：端口分配 + conf 白名单 patch + 深链 +
  日志归档 + `POST /stop` 优雅停止 → pid 兜底强杀
- **调度器 v1**：游戏日界时间窗纯函数、就绪判定/队列（priority desc + FIFO）、指数退避（内存态）、
  双超时 + 看门狗、崩溃恢复（启动清租约）、会话独立 task + 引擎 stop 注入、
  `schedule pause/resume`、SIGINT/SIGTERM 优雅关停
- **控制台 v1**：REST API（见 [`../dev/api.md`](../dev/api.md)）+ WS（`?logs=` tail、`?events=1` 事件流）
  + rust-embed 嵌入 `ui/dist` + 前端七页
- **UID 核验（§9.4）**：切号成功后经 MAA 自定义 pipeline（`--user-resource` + OCR 个人信息页）
  核验登录身份，失败 = `SwitchFailed` 防串数据
- **设备截图**：`DeviceBackend::screenshot()` + `GET /api/devices/{name}/screenshot`
- **发布自动化**：`.github/workflows/release.yml` —— tag `v*` 构建 6 目标安装包 + deb + Arch 包
  + Gentoo ebuild + SHA256SUMS + 自动 Release

**未做**：

- MowerExecutor 的 **DockerRunner（bollard）** —— M1 只有 ProcessRunner
- `mower update` UpdatePlan、`export`/`import`（任务 9）、`migrate` 命令（占位，见 `todo_placeholder.rs`）
- RedroidDocker 设备后端（M2：模板建池/水位扩容）
- OpenAPI/utoipa、统计页 ECharts、调度策略表单编辑
- 72h 真机验收

**测试规模**：`cargo test --workspace` 共 80 个（64 core 单测 + 7 server 测试 + 5 调度场景 + 4 golden）。

## 硬性不变量（违反即拒改）

1. **INV-1 账号切换只许调 MAA**：唯一路径 `switch::execute_switch()` →
   `maa run startup -p default --batch`。**严禁**编写任何登录界面截图识别、坐标点击、
   OCR、输入模拟代码。MAA 不可用 → 任务失败告警，**不降级**。

   > 边界澄清：`DeviceBackend::screenshot()` 是纯 adb 只读截屏（供人看设备画面），
   > **不属** INV-1 禁止的「游戏内自动化」。UID 核验也是 MAA 框架内的 pipeline
   > （`--user-resource` 加载自定义节点），不是自研识别。二者都在设计文档里明确授权。

2. **INV-2 跨平台与可迁移**：不引入平台专属依赖（除非 feature-gated 且默认关）；
   所有状态必须可 export/import。
3. **INV-3 执行器/设备可插拔**：调度器只面向 trait 编程；新执行器不改调度器核心。
4. **INV-4 单一事实源**：账号/设备/策略以工作目录文件树为准；SQLite 只存运行态与统计；
   物化产物可重建（mower bundle 的用户改动除外，见设计文档 §11.4）。

## 开发约定

- **文档先行**：改行为先改设计文档对应章节（或 ADR），再写代码；文档与代码不一致视为 bug。
- Rust：`cargo fmt` + `cargo clippy --workspace --all-targets -- -D warnings` 零告警；
  公共 API 写 doc comment 与示例。
- 提交信息：Conventional Commits（`feat(scheduler): ...`）；**一个 PR 一件事**。
- 新增配置字段：同步更新 `arkreunion-design.md` §11 schema、`user/config.md`、golden 快照。
- 涉及密钥的测试/夹具一律用假值；**永不**把真实 `account_name`/凭据写进仓库。
- 提交前门禁：`cargo fmt && cargo clippy ... -D warnings && cargo test --workspace`
  + `cd ui && npm run build`（含 vue-tsc）。

## 修改热点与注意事项

### `server.rs`（最容易改坏的地方）

这一个文件里有：Router 定义、鉴权中间件、所有 API handler、WS、静态资源回退、
内联测试。改之前先定位：

| 要改什么 | 位置 |
|---|---|
| 路由表 | `fn router()` |
| 鉴权行为 | `fn auth_mw()` |
| 错误响应格式 | `fn actionable()` / 各 handler 内联的 `json!({"error", "hint"})` |
| 静态资源 | `ui_fallback()` |

**鉴权只作用于 `/api/*`** —— 静态资源一并拦截会导致控制台永久白屏
（浏览器 `<script src>` 不带 `?token=`）。改鉴权时先读
`静态资源不经鉴权_控制台可加载` 测试。

**所有错误响应带 `hint`** —— 告诉用户「去哪里改」，不是裸报错。已有测试守着。

### 物化器（`src/materialize/`）

改动必须更新 golden 快照：

```bash
ARKREUNION_UPDATE_GOLDEN=1 cargo test -p arkreunion-core --test golden
git diff crates/arkreunion-core/tests/golden/    # 人工确认只动了白名单字段
```

mower `conf.yml` 的白名单只有 4 个字段：`adb` / `webview.port` /
`webview.token` / `start_automatically`。in/out 对比是「没越界」的证明。

### Store 迁移

改列名/加列要升 `SCHEMA_VERSION` 并在 `migrate()` 里加 `if cur < N` 分支。
**`SCHEMA_V1` 保持原样不改** —— 新库直接建 v1 再靠迁移到最新，
保证「建表」与「迁移」两条路径产出同一 schema。

### 前端

`rust-embed` 在 **release 构建期**嵌入 `ui/dist`，缺 dist 会编译失败（特性）。
改前端后必须：

```bash
cd ui && npm run build    # vue-tsc 类型检查 + vite build
```

## 常见坑（来自本地部署考古）

- mower `conf.yml` 顶层 `account` 字段是 **SMTP 邮箱用户名**，不是游戏账号；
  游戏账号信息在 `skland_info` 与 MAA 侧。
- adb 地址有**双形态**：宿主端口（`127.0.0.1:2771`）与容器网络（`arknights:5555`）。
  物化时按 Runner 形态选择，**写错是高频 bug**。
  arkreunion 自身在容器里时 `host_adb` 也要填容器网络地址（`127.0.0.1` 指向容器自身）。
- mower 数据根由 `MOWER_DATA_DIR` 决定（ProcessRunner 依赖）；DockerRunner 则直接挂载 `/mower/config`。
- 游戏日界是**官服 UTC-4 的 04:00**，不要按本地零点写时间窗逻辑。
- mower alpha 分支会动配置 schema（如 device 段迁移），更新 mower 后先跑 `doctor`。
- `adb exec-out` 与 `adb shell` 的区别：截屏/二进制传输**必须**用 `exec-out`，
  `shell` 会做 CRLF 转换破坏二进制流。
- daemon 重启后 adb server 丢失连接记录，网络设备首次操作必然失败一次；
  内部逻辑都要先 `adb connect`。

## AI 协作流程建议

1. 动手前：读 `arkreunion-design.md` 对应章节 + 相关 ADR + 邻近模块代码。
2. 不确定的设计点：查 `arkreunion-design.md` §22 开放问题；仍未覆盖则**停下提问**，
   不要自行发明架构。
3. 改字段名/配置项：同步「模型 → store → CLI → API → 前端 → 设计文档 → 用户配置手册」
   全链路，并确认是否需要 schema 迁移。
4. 交付时：自述改了哪些模块、测试结果、文档是否同步、**已知限制**。

## 本机部署栈（实机验证用）

`/srv/reunion` —— docker compose 部署（redroid 2781 + arkreunion + mower），配置独立且
git 管理（明文凭据拆分不入库）。用它做真机验证，不要污染 `/srv/arknights` 既有栈。

```bash
cd /srv/reunion && docker compose ps
ark() { docker compose run --rm --workdir /arkreunion \
        --entrypoint /usr/local/bin/arkreunion arkreunion "$@"; }
ark doctor
```

细节见 `/srv/reunion/README.md`。