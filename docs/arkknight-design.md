# arkknight —— 方舟多账号调度中心 · 详细设计文档

| | |
|---|---|
| **版本** | 1.1（补充账号 key 命名与文档对照，见 ADR-0004/0005/0006） |
| **日期** | 2026-10-09 |
| **状态** | 已评审通过（决策记录见 `ai/adr/`，0001~0006） |
| **读者** | 开发者（含 AI 协作者）、架构评审者 |
| **配套文档** | `ai/AGENTS.md`（AI 协作规范）、`dev/`、`user/` |

---

## 目录

1. [项目概述](#1-项目概述)
2. [术语表](#2-术语表)
3. [调研结论与依据](#3-调研结论与依据)
4. [系统架构](#4-系统架构)
5. [硬性设计不变量](#5-硬性设计不变量)
6. [域模型](#6-域模型)
7. [设备层设计](#7-设备层设计)
8. [执行器层设计](#8-执行器层设计)
9. [账号切换设计](#9-账号切换设计)
10. [调度器设计](#10-调度器设计)
11. [配置体系](#11-配置体系)
12. [更新体系](#12-更新体系)
13. [Web 控制台与 API](#13-web-控制台与-api)
14. [持久化设计](#14-持久化设计)
15. [CLI 规范](#15-cli-规范)
16. [跨平台注意事项](#16-跨平台注意事项)
17. [安全考量](#17-安全考量)
18. [可观测性与通知](#18-可观测性与通知)
19. [测试策略](#19-测试策略)
20. [里程碑与验收标准](#20-里程碑与验收标准)
21. [风险登记册](#21-风险登记册)
22. [开放问题](#22-开放问题)
23. [附录](#23-附录)

---

## 1. 项目概述

### 1.1 定位

arkknight 是一个**独立开源项目**：一个跨平台（Linux / Windows / macOS）的多账号明日方舟自动化**编排器**（orchestrator）。它自身不做任何游戏内图像识别与操作自动化，而是编排既有执行器完成多账号调度：

- **MAA**（MaaAssistantArknights）：利用其配置/Profile 体系、账号切换官方能力、任务识别引擎
- **mower**（ArkMowers/arknights-mower）：利用其配置体系与基建排班引擎
- **不限于二者**：执行器抽象（`Executor` trait）可插拔，未来可接入其他助手工具或自定义脚本执行器

项目形态：**Rust workspace**，`arkknight-core`（库）+ `arkknight`（CLI 二进制），rails 式子命令；Web 控制台（Vue3）构建产物嵌入二进制。

### 1.2 目标

- G1 一台模拟器（redroid 或任何 adb 设备，含 Windows 原生模拟器）上运行**多个游戏账号**，按调度策略自动切换
- G2 MAA 与 mower 的**程序与资源独立更新**
- G3 adb 连接地址**灵活管理**（宿主端口 / 容器网络双形态，设备可注册可发现）
- G4 时间冲突时**按需扩容模拟器池**（水位准入，动态决定上限）
- G5 **跨平台**单二进制，配置与统计数据**可导入导出迁移**
- G6 加分项：细粒度跨账号调度（如多账号龙舌兰跑单交叉利用设备空闲窗口）

### 1.3 非目标（第一期明确不做）

- 不实现任何登录界面识别/点击/OCR（见不变量 INV-1）
- 不重写 mower 的基建排班引擎或 MAA 的任务引擎
- 不做模拟器性能监控大盘（只做调度所需的水位探测）
- 不做多用户/多租户（单用户本地工具）

---

## 2. 术语表

| 术语 | 含义 |
|---|---|
| **arkknight** | 本项目/编排器本体 |
| **执行器 (Executor)** | 被编排的自动化工具实例形态（MAA 会话、mower 会话） |
| **设备 (Device)** | 一台可 adb 连接的安卓环境（redroid 容器、MuMu、雷电、物理机……） |
| **会话 (Session)** | 一次「设备租约 + 账号切换 + 执行器运行」的完整生命周期单元 |
| **切号 (Switch)** | 通过 MAA「开始唤醒」任务完成的账号登出→选号→登入 |
| **账号 bundle** | 一个账号的全部私有配置目录（MAA 配置 + mower 配置） |
| **账号 key** | 账号的**本地定位键**（= 目录 `accounts/<key>/`）。本项目内部概念，**非游戏身份** —— 游戏身份是 `account_name`（切号匹配串）与 `uid` |
| **切号匹配串** | `account_name`：MAA 在快速登录列表里的匹配依据（官服=打码手机号片段，B服=昵称） |
| **游戏 UID** | `uid`：游戏内数字 ID，切号后经 MAA OCR 核验登录身份，防登错号串数据（§9.4） |
| **物化 (Materialize)** | 从 arkknight 统一配置生成执行器可直接消费的配置文件的过程 |
| **时间片 (Slice)** | 时间片轮转模式下分配给某账号的一段独占设备时长 |
| **游戏日界** | 官服每日 04:00（UTC-4）刷新；调度时间窗以此为日界 |
| **水位准入** | 扩容模拟器前检查宿主 CPU/内存/GPU 余量是否超过阈值 |
| **亲和矩阵** | 账号×设备的「该账号曾在此设备登录过」记录（切号前提） |

---

## 3. 调研结论与依据

> 详细来源链接见[附录 A](#附录-a调研来源)。

### 3.1 mower（ArkMowers/arknights-mower，alpha 分支）

- **无游戏内账号切换能力**：全仓库无 switch_account 类实现；官方 issue #340 对多账号的建议即「复制目录多实例 + 不同端口」→ arkknight 的「每账号 bundle + 按需会话」与此一致
- 登录相关场景（LOGIN_MAIN/LOGIN_QUICKLY 等）仅用于把游戏带回主界面，**不可用于切号** → 强化 INV-1 的必要性
- 有 `linux.redroid` 设备预设（DeviceProfile/preset_id），设备配置模型含 adb 路径、serial、截图/触控后端、游戏包名
- server 模式：Flask，90+ REST 端点 + `/log` WebSocket。arkknight 依赖的关键端点：`POST /start/<type>`、`POST /stop`、`GET/POST /plan`、`GET/POST/PATCH /conf`、`GET /device/status`、`GET /log`(WS)
- 配置布局：`<data>/config/{conf.yml, plan.json, weekly_plans.yml}`；`@app` 数据根由 `MOWER_DATA_DIR` 环境变量或代码库根决定（ProcessRunner 的关键依据）
- `plan.json` 支持条件副表（backup_plans + Python 表达式 trigger），龙舌兰/但书跑单即由此实现——跨账号跑单交叉调度的信号源
- 更新：上游自带 OTA，但本类部署惯用「git 检出固定 commit + 镜像/进程重建」→ arkknight 统一为 UpdatePlan（§12）

### 3.2 MAA 与 maa-cli

- **账号切换是 MAA 官方能力**：「开始唤醒」(StartUp) 任务参数 `account_name`，行为=启动游戏后在登录界面登出当前账号→按登录名在快速登录列表匹配→登入。官服用打码手机号唯一片段（如 `123****8901`），B 服用昵称；**仅支持该设备上已登录过的账号**；要求片段在已登录账号中唯一
- maa-cli：`MAA_CONFIG_DIR` 下 `profiles/`（连接配置：adb 地址、触控/截图方式）与 `tasks/`（任务 + 按时间/日期/活动的条件变体）；`maa run <task> -p <profile> --batch` 非交互运行；TOML/YAML/JSON 三格式
- 更新体系天然分离：`maa self update`（CLI 自身）、`maa install/update`（MaaCore）、资源 git 热更新（`resource.remote.url`，可配代理）
- 官服按东四区处理（游戏每日 04:00 开始）→ 调度器游戏日界依据

### 3.3 redroid 与模拟器多实例

- 多实例标准做法：每实例独立容器 + 独立数据卷 + 独立宿主端口（容器内固定 5555）；`docker compose start/stop` 或 Docker API 按需启停
- 本机现有可复用资产（一期验证环境）：redroid 容器 `arknights2771`（宿主 `127.0.0.1:2771`、容器网络 `arknights:5555`、nvidia GPU、1920x1080@280、libndk 转译、watchdog 注入）；`redroid-script/`（镜像补丁工具）、`2771-base/install/update-日期` 镜像快照约定、`ak.url`（官方 APK 下载源）、`refill-akdata.sh`（app 数据备份回填）
- 每实例开销大（GPU + 最高 12GB 内存）→ 水位准入门控扩容

### 3.4 参考项目

- AUTO-MAS（Windows 向 MAA 多账号托管）：借鉴其「队列 + 配置切换 + 日志监看」思路；其 Windows 桌面绑定与 .NET 栈不适配本需求，不自依赖

---

## 4. 系统架构

### 4.1 架构总览

```
┌─────────────────────────────────────────────────────────────────────┐
│  Web 控制台（Vue3 + Vite 构建产物，rust-embed 嵌入单二进制；PWA）      │
│  账号 / 设备 / 会话 / 调度队列 / 统计 / 维护 / 深链 mower UI          │
├─────────────────────────────────────────────────────────────────────┤
│  axum REST + WebSocket（utoipa 自动生成 OpenAPI，token 鉴权）         │
├─────────────────────────────────────────────────────────────────────┤
│  CLI（clap 子命令，rails 式）── daemon 模式走本地 API，               │
│                               standalone 模式直调库（文件锁互斥）     │
├─────────────────────────────────────────────────────────────────────┤
│  调度器（tokio）：策略引擎 → 会话队列 → 设备租约 → 切号 → 生命周期    │
├──────────────────────┬──────────────────────────────────────────────┤
│  Executor trait      │  MaaCliExecutor   切号 + MAA 任务             │
│  （可插拔）           │  MowerExecutor    基建会话（时间片）           │
│                      │   └─ Runner 子层: DockerRunner │ ProcessRunner │
│                      │  （预留：ScriptExecutor 等第三方）             │
├──────────────────────┼──────────────────────────────────────────────┤
│  DeviceBackend trait │  External   仅 adb 地址（一期；跨平台，        │
│  （可插拔）           │             覆盖 MuMu/雷电/物理机/redroid）    │
│                      │  RedroidDocker  bollard 建池/扩容（二期）      │
├──────────────────────┴──────────────────────────────────────────────┤
│  配置层：统一配置（文件树，单一事实源）                                │
│   ├─ 物化器：Account → MAA profile/tasks（TOML）                     │
│   │           Account → mower bundle（conf.yml/plan.json/...）       │
│   ├─ 运行期改写：设备 serial / webview 端口（按 Runner 形态选地址）   │
│   └─ 导入器：现有 mower conf.yml、MAA gui.json 反向导入               │
├─────────────────────────────────────────────────────────────────────┤
│  持久化：SQLite（WAL：会话/事件/统计/端口分配/亲和矩阵，版本化 schema）│
│  日志：tracing → 文件轮转 + 会话日志归档 + SQLite 索引                │
└─────────────────────────────────────────────────────────────────────┘
```

### 4.2 Crate 结构（Rust workspace）

```
arkknight/
├── Cargo.toml            # workspace
├── crates/
│   ├── arkknight-core/       # 领域模型、配置、物化、调度器、执行器、设备后端、持久化（库）
│   │   └── src/
│   │       ├── model/        # Account/Device/Session/SchedulePolicy/...
│   │       ├── config/       # arkknight.toml 解析、文件树布局、导入导出
│   │       ├── materialize/  # MAA/mower 配置物化与运行期改写
│   │       ├── device/       # DeviceBackend trait + external/ + redroid/(M2)
│   │       ├── executor/     # Executor trait + maa/ + mower/（runner: docker|process）
│   │       ├── scheduler/    # 策略引擎、队列、状态机、看门狗、水位(M2)、事件(M3)
│   │       ├── store/        # SQLite 访问层 + 迁移
│   │       └── switch/       # 切号编排（唯一入口：调 MAA）
│   └── arkknight/           # CLI + axum server + rust-embed 前端（二进制）
├── ui/                   # Vue3 + Vite 前端源码
└── docs/                 # 三受众文档（本目录）
```

### 4.3 运行形态：daemon 与 standalone 双模式

- **daemon 模式**：`arkknight server` 常驻，持有调度器与全部写操作。启动后在 `state/daemon.json` 记录 `{pid, port, started_at}`
- **standalone 模式**：未检测到 daemon 时，写类 CLI 命令直接调用 arkknight-core，**必须先获取工作目录排他文件锁**（`state/.workdir.lock`，flock）；daemon 存活时 CLI 一律走本地 HTTP API，避免双写
- 只读命令（`status`/`list` 类）始终可直接读文件 + SQLite（WAL 允许并发读）
- 规则：**daemon 运行期间，任何非 API 写路径都是 bug**（开发期 CI 用集成测试守护该约束）

---

## 5. 硬性设计不变量

> 违反任一条的 PR/改动直接拒绝。AI 协作者必须遵守（同时写入 `ai/AGENTS.md`）。

**INV-1 账号切换 100% 调用 MAA 官方能力，绝不造轮子。**
- 唯一实现：`switch::run_switch()` → `maa run startup -p <account-profile> --batch`（StartUp 任务 + `account_name` 参数）
- 禁止出现：登录界面截图识别、坐标点击、OCR、输入法模拟等任何自研登录自动化代码
- MAA 不可用/切号失败 → 会话进入 `SwitchFailed` 并告警，**不降级**为其他手段
- 理由：登录界面改版频繁，MAA 资源由上游持续维护；自研必然腐化

**INV-2 跨平台与可迁移。**
- 发布物为三平台单二进制（Linux x86_64/aarch64、Windows x86_64、macOS aarch64），无运行时依赖（platform-tools/模拟器属外部依赖，`doctor` 探测）
- 全部状态可经 `arkknight export/import` 迁移：配置树 + SQLite + manifest（格式版本）；不允许出现导不走的隐式状态

**INV-3 执行器可插拔。**
- 调度器只面向 `Executor` trait 编程；新增执行器不得修改调度器核心
- 同理设备只面向 `DeviceBackend` trait

**INV-4 单一事实源。**
- 账号/设备/策略的唯一事实源是 arkknight 工作目录文件树；物化产物（MAA TOML、mower bundle）可随时由事实源重建（mower bundle 的用户改动除外，见 §11.4 双向规则）
- SQLite 仅存运行态与统计，可由文件树 + 日志重建的部分不进 SQLite

---

## 6. 域模型

### 6.1 Account（文件：`accounts/<key>/account.toml`）

> **命名约定**：`Account` 的四个标识字段各司其职，勿混——
> `key` 是**本地定位键**（本项目内部概念：目录名 + CLI 参数），
> 而 `account_name` / `uid` 是**游戏侧标识**，`display_name` 仅供展示。
> 命名决策与备选（`slot`/`profile`/`name` 为何不用）见 [ADR-0004](./ai/adr/0004-账号本地标识用key.md)。

| 字段 | 类型 | 说明 |
|---|---|---|
| `key` | string | 本地定位键（目录名，slug）；游戏身份不由它表达 |
| `display_name` | string | 展示名 |
| `server` | `official \| bilibili` | 服务器类型（影响 client_type/包名/资源） |
| `account_name` | string | **MAA 切号匹配串**（官服=打码手机号片段，B服=昵称；须全局唯一，`doctor` 校验） |
| `uid` | string? | 游戏 UID（纯数字）。配置后切号成功即跑 UID 核验（§9.4）；缺省跳过核验（doctor 告警串数据风险） |
| `enabled` | bool | 禁用后不参与调度 |
| `schedule.windows` | `[{start,end,executor,task?}]` | 游戏日界内的每日时间窗；executor ∈ `mower\|maa`；maa 窗口可指定 `task`（accounts/<key>/maa/tasks/ 下的任务名，缺省拒绝调度 maa 窗口）；窗口须 start<end（不跨午夜，校验拒绝） |
| `schedule.priority` | int 0-100 | 队列优先级，默认 50 |
| `schedule.slice` | duration | 时间片长度，覆盖全局默认（默认 2h） |
| `schedule.runner` | `process \| docker` | mower 会话运行形态（ADR-0001 D5），默认 `process`；MAA 会话不受影响 |
| `provisioned_on` | `[device]` | 已人工登录过的设备（亲和的种子数据，正式记录在 SQLite） |

### 6.2 Device（文件：`devices/<name>.toml`）

| 字段 | 类型 | 说明 |
|---|---|---|
| `name` | string | 唯一标识 |
| `backend` | `external \| redroid` | external=已存在只连不管；redroid=arkknight 全生命周期管理（M2） |
| `connection.host_adb` | string | 宿主视角地址（`127.0.0.1:2771`），ProcessRunner / 宿主侧 maa-cli 使用 |
| `connection.docker_adb` | string? | 容器网络视角地址（`arknights:5555`），DockerRunner 使用 |
| `connection.docker_network` | string? | DockerRunner 需加入的网络 |
| `redroid.*` | object | M2：镜像标签、端口段、数据卷、GPU、资源上限等模板 |
| `notes` | string | 备注 |

> **双地址设计**（审查完善点）：同一设备对宿主进程与容器内进程呈现不同 adb 地址。物化器按目标 Runner 形态选择 `host_adb` 或 `docker_adb`；二者都填时才算「Docker 兼容」。

### 6.3 Session（运行态，持久化于 SQLite）

| 字段 | 说明 |
|---|---|
| `id` | 自增 |
| `account_key` / `device_name` / `executor` | 归属 |
| `runner` | `docker \| process`（mower 会话的运行形态） |
| `state` | 状态机见 §10.2 |
| `slice_deadline` / `max_runtime_deadline` | 两个超时源 |
| `mower_port` | 分配的 mower Web UI 端口（深链用） |
| `started_at` / `ended_at` / `outcome` / `error` | 结果 |

### 6.4 SchedulePolicy（全局，`arkknight.toml [scheduler]`）

见 §11.2 配置 schema；含默认时间片、最大会话时长、退避参数、切号重试次数、游戏日界等。

### 6.5 关系约束

- 一个设备同一时刻**至多一个活跃 Session**（设备租约）
- 一个账号同一时刻**至多一个活跃 Session**（任意设备）
- 切号只能发生在持有设备租约的会话流程内（手动 `arkknight switch` 也要短暂持有租约）

---

## 7. 设备层设计

### 7.1 DeviceBackend trait（M1 版签名，允许 M2 前修订）

```rust
#[async_trait]
pub trait DeviceBackend: Send + Sync {
    fn name(&self) -> &str;
    /// 设备是否在线可达（adb connect + devices 校验）
    async fn health(&self) -> Result<DeviceHealth, DeviceError>;
    /// 宿主/容器双视角地址（物化器消费）
    fn endpoints(&self) -> DeviceEndpoints; // { host_adb, docker_adb, docker_network }
    /// ---- 以下 M2（RedroidDocker 实现；External 返回 Unsupported）----
    /// 启动一个新实例（模板实例化），返回设备名
    async fn provision(&self, spec: RedroidSpec) -> Result<String, DeviceError>;
    async fn start(&self) -> Result<(), DeviceError>;
    async fn stop(&self) -> Result<(), DeviceError>;
    async fn destroy(&self) -> Result<(), DeviceError>;
    /// 宿主水位（CPU/内存/GPU 余量）——扩容准入用
    async fn host_watermark(&self) -> Result<Watermark, DeviceError>;
}
```

### 7.2 External 后端（一期）

- 只依赖 `adb_path`：`adb connect <host_adb>` → `adb devices` 校验；不做生命周期管理
- 天然跨平台：Windows 下 `host_adb` 指向 MuMu/雷电等的 adb 端口即可用（M1 即可服务 Windows 用户，DockerRunner 缺席时用 ProcessRunner）
- `device test` 命令 = health 探测 + 游戏包检测（`pm list packages`）

### 7.3 RedroidDocker 后端（二期，仅 Linux）

- bollard 创建容器：镜像标签（如 `Lax/mrfz:2771-update-*`）、宿主端口段 28000-28099 分配、named volume 持久化登录态、nvidia runtime、watchdog/init.rc bind（复用现有资产）
- **水位准入**：provision 前检查 `host_watermark()`（内存 ≥ 模板 mem_limit + headroom；CPU 空闲；GPU 显存可选）→ 不满足则排队而非扩容（动态上限，无人为台数上限）
- **亲和矩阵**：SQLite `logins(account_key, device_name, status, last_verified_at)`；调度时优先分配已登录设备；未登录则可触发预置任务（§9.3）

---

## 8. 执行器层设计

### 8.1 Executor trait

```rust
#[async_trait]
pub trait Executor: Send + Sync {
    fn kind(&self) -> ExecutorKind; // Maa | Mower
    /// 启动执行器会话（假定账号切换已完成、设备租约已持有）
    async fn start(&self, ctx: &SessionCtx) -> Result<ExecutorHandle, ExecutorError>;
    /// 优雅停止（mower: POST /stop → 等待退出；MAA: 等任务进程结束/kill）
    async fn drain(&self, handle: &ExecutorHandle, grace: Duration) -> Result<(), ExecutorError>;
    /// 健康探测（喂看门狗用）
    async fn health(&self, handle: &ExecutorHandle) -> ExecutorHealth;
}
```

### 8.2 MaaCliExecutor

- 每账号独立 `MAA_CONFIG_DIR = <workdir>/accounts/<key>/maa/`（内含 `profiles/default.toml` + `tasks/*.toml`），完全隔离、无跨账号共享文件
- 运行 = `maa run <task> -p default --batch`，子进程管理（tokio::process），stdout/stderr 实时入会话日志
- MaaCore 本体与热更新资源由 maa-cli 全局安装目录统一管理（多账号共享，按 server 类型区分 resource 差异走 profile 的 `resource.*` 字段）
- 用途：a) 切号（§9，任务=startup）；b) 独立 MAA 任务会话（周计划刷图、肉鸽等，用户自定义 tasks/*.toml）

### 8.3 MowerExecutor 与 Runner 子层（审查完善点）

mower 会话的实际运行形态由 **Runner** 决定，两种实现，接口一致：

| | DockerRunner | ProcessRunner |
|---|---|---|
| 形态 | bollard 起容器（镜像由用户 Dockerfile 或现有 `Dockerfile.mower` 系构建） | 本地 mower 检出 + Python 环境，spawn `python run_server.py` |
| 配置注入 | 挂载 `accounts/<key>/mower` → `/mower/config`（**读写**，UI 改动直接持久化）；tmp 卷按会话分配 | `MOWER_DATA_DIR = accounts/<key>/mower-data/`（内含指向 `../mower` 的 config 符号链接与 tmp/） |
| adb 地址 | `docker_adb`（容器网络） | `host_adb` |
| 适用 | Linux 服务器/Docker Desktop | 任意平台、无 Docker 场景（Windows 原生跑 mower） |
| 一期状态 | **M1 优先实现**（贴合现有验证环境） | M1 并行交付（跨平台承诺） |

- 会话启动前物化器改写 bundle 内 `conf.yml`：`adb` 字段（按 Runner 选地址）、`webview.port`（分配 58100-58199）、`start_automatically: true`
- 端口分配：SQLite `port_allocations` 记账 + 绑定前探测空闲
- 深链：控制台展示 `http://<host>:<port>/?token=<token>` 直达该账号 mower 自带 UI（配置编辑走 mower，分阶段吸收，见 §13.1）
- 会话结束：`POST /stop` 优雅退出（超时 grace 后强停容器/进程）；tmp 数据（report.csv、data.db）留在账号目录 `mower-data/tmp/`，供统计导入

---

## 9. 账号切换设计

### 9.1 标准流程（唯一实现路径）

```
持有设备租约
  ├─ 若设备上有活跃会话 → drain（优雅停止）
  ├─ adb shell am force-stop com.hypergryph.arknights[.bilibili]
  ├─ 物化器改写 MAA profile 的 connection.address（按 Runner/宿主视角）
  └─ maa run startup -p default --batch   ← MAA 官方切号（INV-1）
       └─ 成功判定：maa-cli 退出码 0 且任务链完成
            ├─ 成功 → 移交执行器（mower 启动时检测已登录，直接续跑）
            └─ 失败 → 重试 ≤ max_switch_retries（间隔退避）→ 仍败则 Session=SwitchFailed + 告警
```

- 耗时预算：单次约 1-2 分钟；`switch_log` 表记录每次耗时与 maa 日志摘录（前 200 行）
- **账号唯一性校验**：`doctor` 与 `account add` 时校验 `account_name` 在同设备已登录账号中的唯一性提示（最终以 MAA 运行结果为准）

### 9.2 亲和前提

切号仅对「该账号曾在此设备登录过」的设备有效（MAA 只能选快速登录列表中的账号）。

### 9.4 UID 核验（防登错号串数据）

**背景**：MAA 的 `account_name` 切号是「在快速登录列表按登录名查找」，匹配失败时可能
静默跳过切换、留在旧账号继续执行——这是 MAA 已知未解问题（issue #15309「登录时未切换
账号，导致在错误的账号执行定时计划」）。MAA 本身**没有**登录后 UID 核验能力。

**方案（MAA 框架内实现，INV-1 合规——核验不触碰登录自动化）**：

1. 账号配置 `uid`（provision 人工登录时录入 / `account add --uid` / 控制台 PATCH）
2. 物化器在 `accounts/<key>/maa/resource/pipeline/arkknight_uid_check.json` 生成核验
   pipeline（主界面点头像 → 个人信息页 OCR 期望 UID → 点返回；roi 以 1280x720 基准，
   MaaCore 自动缩放，首次使用建议校准）+ `tasks/uid_check.toml`
3. 切号流程：startup 成功后运行 `maa run arkknight-uid-check -p default --batch
   --user-resource`（单次，不参与切号重试）
4. **核验失败 = 切号失败**（`SwitchFailed` + 退避 + 告警）：宁可不开会话也不在错误
   账号上执行基建操作；未配置 uid 的账号 doctor 告警串数据风险，切号不核验

### 9.3 预置流程（provision，每账号每设备一次性）

1. `arkknight provision <account> --device <d>`（或控制台按钮）
2. arkknight 确保设备在线、游戏已安装；输出投屏指引（ws-scrcpy 链接或 `adb shell am start` 拉起登录界面）
3. 等待用户在投屏中**人工登录一次**该账号（可勾选「记住/快速登录」），用户回车/点按钮确认
4. arkknight 记录 `logins(account, device, provisioned)`，此后可自动切号
5. 预置期间设备租约同样被持有（与调度互斥）

---

## 10. 调度器设计

### 10.1 三期演进

| 期 | 能力 | 说明 |
|---|---|---|
| M1 | 单设备串行 + 静态时间窗 | External 设备；按账号时间窗/优先级排队轮转；手动会话可插队（priority=100） |
| M2 | 多设备池 + 水位扩容 | RedroidDocker 动态建池；亲和优先分配；水位准入动态决定并发上限 |
| M3 | 事件驱动交叉调度 | 监听 mower 空闲信号提前释放时间片；跑单时敏窗口优先锁设备 |

### 10.2 会话状态机

```
Created → Queued ─(获得设备租约)→ Switching → Running ─┬─(slice 到期/手动)→ Draining → Finished
                        │                 │             ├─(max_runtime/看门狗)→ Draining → TimedOut
                        │                 ├─ 切号重试耗尽 → SwitchFailed
                        │                 └─ 执行器崩溃不可恢复 → Failed
                        └─ 取消 → Cancelled
任何状态 → Cancelled（手动）
```

- **设备租约（lease）**：SQLite 事务获取 `device_leases(device_name, holder, acquired_at, heartbeat_at)`；daemon 启动时按单写者语义**全清现存租约**（持锁启动 ⇒ 现存租约必属死进程），CLI 场景按心跳超时保守回收
- **双超时**：`slice_deadline`（正常轮转）与 `max_runtime_deadline`（硬上限，默认 6h）独立计时
- **看门狗**：执行器 health 探测（mower=WS 日志心跳 + `/device/status`；MAA=子进程存活）连续 N 次失败 → Draining → 按退避重排
- **退避**：连续失败按 `{initial:5m, factor:2, max:60m}` 指数退避该账号的下次调度，避免「空扫死锁」类问题（吸取 mower issue 草稿教训：失败任务必须退避，不得 5 分钟空转重试）
- **签到保护窗**（M1 简化实现）：每账号时间片内，mower 自身负责森空岛签到等；arkknight 保证每账号每日至少一个完整时间片（`daily_guarantee = true` 默认开）
- **M1 实现备注**（实现与设计的差异备案）：退避状态为 daemon 内存态（重启清零，事件留痕于
  `session_events`）；时间窗不跨午夜（start<end，配置校验拒绝）；`daily_guarantee` 仅告警
  （游戏日内有窗口但零会话时记录事件，不主动补跑，避免与窗口语义冲突）；时间窗可带 `task`
  字段指定 maa 任务（§6.1）

### 10.3 排队与优先级（M1 算法）

- 就绪集合 = enabled ∧ 当前处于某时间窗 ∧ 无活跃会话 ∧ 未在退避
- 单设备：按 `(priority desc, 就绪等待时长 desc)` 选出下一个；时间片轮转，同优先级 FIFO 保证公平
- 时间窗之间无交集的账号天然错峰；有交集则排队（这正是多账号单设备的预期行为）
- 手动 `session start` / `switch` 以 priority=100 插队，但**不抢占**已 Running 会话（等 Draining）

### 10.4 M3：跨账号交叉调度（加分项设计预案）

- 信号源：mower `/log` WebSocket 的事件模式集（版本化的 pattern 清单，如基建空闲 `remaining_time > 540s`、跑单任务完成）
- **提前释放**：A 账号 mower 进入长空闲 → 调度器收到信号 → 优雅 Draining → B 账号插位 → A 的下次任务前切回（依据 mower 下次任务时间，M3 spike 验证可获取性，见开放问题 Q2）
- **跑单锁定**：识别龙舌兰/但书跑单窗口（plan.json backup_plans trigger 特征 + 日志），在窗口内提升该会话优先级并锁定设备不轮转
- 全部为**启发式增强**，可一键关闭（`scheduler.cross_account = false` 时回到纯时间片）

---

## 11. 配置体系

### 11.1 工作目录布局

```
<workdir>/
├── arkknight.toml                # 主配置（全局策略/路径/服务）
├── devices/<name>.toml       # 设备注册表
├── accounts/<key>/
│   ├── account.toml          # 账号身份 + 调度策略（§6.1）
│   ├── maa/                  # MAA_CONFIG_DIR（profiles/default.toml + tasks/*.toml）——物化产物+用户自定义任务
│   ├── mower/                # mower 配置 bundle（conf.yml/plan.json/weekly_plans.yml）——物化产物+用户通过深链 UI 的改动
│   └── mower-data/           # ProcessRunner 的 MOWER_DATA_DIR（config→../mower 符号链接 + tmp/）
├── state/
│   ├── arkknight.db              # SQLite（WAL）
│   ├── daemon.json           # daemon 运行信息
│   └── .workdir.lock         # standalone 写互斥 flock
├── logs/                     # arkknight 日志 + 会话日志归档 logs/sessions/<session-id>.log
├── export/                   # export 产物默认目录
└── templates/                # 物化模板（mower conf 模板、MAA profile 模板，可用户覆盖）
```

### 11.2 arkknight.toml schema（v1 草案）

```toml
schema_version = 1

[paths]
maa_dir   = "~/.local/share/arkknight/maa"     # maa-cli 安装根（doctor 可自动安装）
mower_dir = "~/src/arknights-mower"        # mower 检出（ProcessRunner 用）
adb_path  = "adb"
docker_mower_image = "arkknight-mower:latest"  # DockerRunner 用
docker_host = ""                            # Docker 端点；空=本机默认（unix socket/命名管道）；
                                            # 容器内部署指向 socket-proxy（tcp://socket-proxy:2375）

[server]
bind  = "127.0.0.1"
port  = 7100
token = ""                                  # 非空时启用鉴权（Bearer 或 ?token=）

[scheduler]
timezone            = "Asia/Shanghai"       # 展示时区（调度计算用游戏日界）
game_day_boundary   = "04:00"               # 游戏日界（官服 UTC-4 的本地表示）
default_slice       = "2h"
max_session_runtime = "6h"
max_switch_retries  = 2
daily_guarantee     = true
drain_grace         = "2m"                  # 会话优雅停止等待（超时强杀）
watchdog_interval   = "30s"                 # 执行器健康探测间隔
watchdog_threshold  = 3                     # 连续失败次数 → Draining
backoff             = { initial = "5m", max = "60m", factor = 2.0 }
cross_account       = false                 # M3 开关

[ports]
mower_session_range = [58100, 58199]
redroid_host_range  = [28000, 28099]        # M2

[device_defaults.redroid]                   # M2 池模板
image        = "Lax/mrfz:2771-update-260914"
mem_limit_gb = 12
cpu_limit    = 8
gpu          = true
watermark    = { free_mem_gb = 14, cpu_idle_pct = 20 }   # 扩容准入水位
```

### 11.3 物化器（Materializer）

输入：account.toml + 设备端点 + Runner 形态 + 模板。输出与运行期改写：

| 目标 | 生成/改写内容 |
|---|---|
| MAA profile | `profiles/default.toml`：`connection.address = <host_adb 或按需>`、`instance_options.touch_mode = "ADB"`（redroid 场景稳定）等；切号前每次改写 address |
| MAA tasks | `tasks/startup.toml`：`type="StartUp"`, `params={client_type, account_name, start_game_enabled=true}`；用户可追加自定义任务文件 |
| mower bundle | 初始化=模板复制（或导入现有 conf.yml）；每次会话启动前改写 `conf.yml` 的 `adb`、`webview.port`、`start_automatically` 三个字段（serde_yaml 语义级白名单 patch——白名单外键值原样保留，注释/键序不保真，见 `ai/adr/0002`） |

幂等性：物化是纯函数（除上述白名单字段），重复执行结果一致。

### 11.4 双向规则（审查完善点：防用户改动丢失）

- mower bundle 内 `conf.yml/plan.json/weekly_plans.yml` 的**用户改动（含通过深链 mower UI 的编辑）是事实源的一部分**——因为 DockerRunner 直接读写挂载、ProcessRunner 符号链接直指，天然持久化，**无需回拷同步**
- arkknight 只白名单改写 3 个设备相关字段（§11.3），且**仅在会话启动瞬间**改写；若用户在会话运行中通过 mower UI 改了这 3 个字段，以用户为准，会话结束前不再覆盖（记入会话事件日志）
- MAA tasks/profiles：物化产物可重建；`tasks/` 内用户自定义文件永不触碰

### 11.5 导入导出（INV-2）

- `arkknight export [--out arkknight-export-YYYYMMDD.zip] [--redact-secrets]`
  - zip 内容：`manifest.json`（arkknight 版本、schema_version、导出时间、redacted 标记、内容清单+哈希）+ `arkknight.toml` + `devices/` + `accounts/`（含 maa/mower 配置与 mower-data/tmp 中的 report.csv 统计源）+ `state/arkknight.db`
  - `--redact-secrets`：按字段名清单脱敏（`skland_info[].password/account`、`pass_code`、`account`（邮箱）、token 类），脱敏文件导入后提示补填
- `arkknight import <zip> [--merge|--replace]`：校验 manifest 与 schema_version → 高版本拒绝并提示升级 arkknight → 跨机/跨平台落地
- 统计另可单独导出 CSV/JSON（`arkknight export --stats-only`）

### 11.6 反向导入器（存量迁移）

- `arkknight account import-mower --conf <现有conf.yml> --plan <plan.json>`：生成账号 bundle（模板=存量文件），account_name 留空待补
- `arkknight account import-maa --gui-json <gui.json>`：从 MAA GUI 多配置中按配置名生成各账号的 MAA tasks/profiles（M1 末交付，优先级低于主链路）

---

## 12. 更新体系（三类 UpdatePlan，均程序/资源分离）

| 对象 | 程序更新 | 资源更新 | 触发 |
|---|---|---|---|
| **MAA** | `maa self update`（CLI）+ `maa update`（MaaCore） | maa-cli 资源 git 热更新（`[resource] auto_update` 或手动） | `arkknight maa update [--resource-only]`；控制台按钮 |
| **mower** | DockerRunner=重建镜像（用户 Dockerfile，pin commit）；ProcessRunner=`git fetch + checkout FETCH_HEAD`（detach） | 无独立资源（随代码） | `arkknight mower update [--ref <commit/tag>] [--runner process\|docker]`、`mower rollback`、`mower version`；默认跟踪 alpha，pin 记录落 `state/mower-pin.toml`；有活跃会话或 daemon 存活时拒绝更新 |
| **模拟器镜像/APK**（M2） | `redroid-script` 构建基础镜像 | 流水线：装 APK（`ak.url` 最新版）→ 进游戏下载资源 → `docker commit` 新日期标签 → 设备滚动重建（避让活跃会话） | 手动 `arkknight device upgrade-image`；游戏版本更新后执行 |

约束：所有更新操作必须**避让活跃会话**（等待或要求先 drain）；更新动作与结果记入 SQLite 维护日志。

---

## 13. Web 控制台与 API

### 13.1 前端方案（评估结论，矩阵全文）

评估维度：生态成熟度、与既有栈一致性、数据密集后台适配（表格/甘特/时间线）、嵌入 Rust 二进制、移动端覆盖、长期维护。

| 方案 | 优势 | 劣势 | 结论 |
|---|---|---|---|
| **Vue3 + Vite**（选定） | mower 自带 UI 即 Vue（栈对齐、心智一致）；Element Plus/Naive UI 成熟、中文文档一流；ECharts 统计图表齐备；构建产物静态化易 rust-embed；PWA 一行配置覆盖手机浏览器 | 大型复杂交互生态略逊 React | **选定**：Naive UI（TS 友好、树摇好）+ ECharts + Pinia + Vue Router |
| React + Vite | 生态最大；antd 中文后台事实标准；未来 RN 复用 hooks 层 | 与 mower 栈不一致；本期无 RN 交付 | 备选 |
| Svelte 5 / SolidJS | 运行时最轻、性能好 | 表格/图表/排程组件生态薄、中文资料少，维护押注个人 | 备选 |
| React Native / Flutter | 手机原生体验最佳 | 无法嵌入二进制、发布链路重、需另维护 API 兼容层 | 仅作为未来「手机伴侣 App」候选（复用本 REST/WS API），不作主控制台 |

一期页面清单：Dashboard（总览+队列）｜账号管理（含预置引导向导）｜设备管理（health/水位/测试/截图）｜会话列表（实时状态+深链 mower UI）｜调度策略编辑｜维护中心（三类更新+doctor）｜统计（ECharts）｜日志查看器（WS 实时 tail）。

> **已实现对照**（2026-10-09）：七页已交付（总览/会话/日志实时/账号 CRUD/设备测试+截图/调度只读/维护 doctor）。**调度策略编辑、统计页、账号预置引导向导** 属尾批未做；「深链 mower UI」通过会话分配的 `mower_port` 实现。页面与 API 的实际清单见 [`dev/api.md`](./dev/api.md)。

**配置界面策略（分阶段吸收）**：M1 只做 arkknight 自有配置（账号/设备/策略）表单 + 深链 mower 自带 UI（会话运行期可用）+ MAA tasks 只读展示；M2 起把高频 mower 配置项吸收进统一界面并回写 bundle；plan.json 可视化编辑器（17 房间排班）远期评估，不自研轮子优先深链。

### 13.2 REST API 草案（utoipa 注解自动生成 OpenAPI，`/docs` 挂 Swagger UI）

```
GET    /api/status                       # 总览：账号/设备/会话/队列/水位
# 账号
GET    /api/accounts                     POST /api/accounts
GET    /api/accounts/{key}   PATCH/DELETE
POST   /api/accounts/{key}/provision      # 预置（返回投屏指引，WS 推进度）
# 设备
GET    /api/devices                      POST /api/devices
GET    /api/devices/{name}  PATCH/DELETE
POST   /api/devices/{name}/test          # adb 探测
POST   /api/devices/{name}/watermark     # 水位（M2）
# 切号与会话
POST   /api/switch        {account_key, device?}
GET    /api/sessions                     POST /api/sessions {account_key, executor, slice?}
DELETE /api/sessions/{id}                POST /api/sessions/{id}/drain
# 调度
GET    /api/schedule                     POST /api/schedule/pause | /api/schedule/resume
# 维护
POST   /api/updates/maa    {resource_only}
POST   /api/updates/mower  {ref?}
GET    /api/doctor                       # 体检报告
# 数据
GET    /api/logs?session=&tail=          GET /api/stats?account=&from=&to=
GET    /api/export?redact=1              POST /api/import (multipart)
# 实时
WS     /api/ws                           # 事件流：会话状态机迁移/切号进度/日志 tail/水位
```

鉴权：`token` 非空时 Bearer 或 `?token=`（与 mower 兼容习惯）；默认 bind 127.0.0.1。

> **本节是草案，含未实现项。** 实际已实现的路由见 [`dev/api.md`](./dev/api.md)（那里逐条标注了「未实现」）。
> 关键差异：鉴权**只作用于 `/api/*`**，静态资源放行 —— 见 [ADR-0005](./ai/adr/0005-Web控制台鉴权只作用于API.md)。
> 容器化部署时 `bind` 须设 `0.0.0.0`（容器内绑 loopback 收不到宿主端口转发），此时按 §17 强制配 token。

---

## 14. 持久化设计

SQLite（rusqlite + WAL + bundled），`state/arkknight.db`，schema 版本化（`meta.schema_version` + `PRAGMA user_version`，内建迁移器，`arkknight migrate` 手动触发）。**时间戳列统一为 INTEGER（UTC epoch 毫秒）**；Store 为单连接 + Mutex（rusqlite Connection 仅 Send，包 Mutex 后 Store: Sync）。

```sql
-- 运行态（可重建部分不入库，INV-4）；实现基线见 crates/arkknight-core/src/store/mod.rs SCHEMA_V1
sessions(id PK, account_key, device_name, executor, runner, state,
         mower_port, locator,                    -- locator=`pid=<n> port=<p>`，跨进程 stop 依赖
         slice_deadline_ms, max_runtime_deadline_ms,
         started_at_ms, ended_at_ms, outcome, error)
session_events(id PK, session_id FK, ts_ms, kind, detail)      -- 状态机迁移/看门狗/退避（任何迁移必写）
device_leases(device_name PK, holder, acquired_at_ms, heartbeat_at_ms)
               -- holder ∈ 'session:<id>' | 'switch:<account>' | 'provision:<account>'：
               -- 手动 switch/provision 同样短暂持有租约（§6.5），不伪造会话行
port_allocations(port PK, holder, kind, allocated_at_ms)       -- 分配前绑定探测（R8）
switch_log(id PK, ts_ms, account_key, device_name, ok, duration_ms, retries, maa_log_excerpt)
logins(account_key, device_name, status, first_at_ms, last_verified_ms, PRIMARY KEY(account_key, device_name))
maintenance_log(id PK, ts_ms, target, action, from_version, to_version, ok, detail)
-- 统计
stats_daily(day TEXT, account_key, minutes_run, sessions_cnt, switch_cnt, sanity_spent,
            PRIMARY KEY(day, account_key))   -- 分钟数来自 sessions；明细来自各 bundle report.csv 导入
```

日志：tracing 分层（arkknight 自身 → `logs/arkknight.log` 轮转；会话 stdout/stderr → `logs/sessions/<id>.log` 归档，SQLite 只存索引与尾部摘录）。

---

## 15. CLI 规范

```
arkknight [--workdir DIR] [--maa-dir DIR] [--mower-dir DIR] [--config FILE] [-v] <COMMAND>
```

全局参数：`--workdir`（默认：cwd 向上逐级探测 `arkknight.toml`）；`--maa-dir/--mower-dir` 覆盖 arkknight.toml `[paths]`；`-v` 提升 verbosity。

| 命令 | 参数 | 说明 |
|---|---|---|
| `init` | `--dir` | 初始化工作目录（目录骨架 + 交互式探测 adb/maa/mower/docker 写入 arkknight.toml） |
| `doctor` | | 环境体检：adb 可用与设备连通、maa-cli 版本与 MaaCore、mower 检出/镜像、Docker 可用性、account_name 唯一性、端口段空闲 |
| `server` | `--port --bind --open` | 启动 daemon + Web 控制台（`--open` 自动开浏览器） |
| `status` | | 总览（账号/设备/活跃会话/队列/退避） |
| `account` | `add \| list \| show \| remove \| enable \| disable \| import-mower \| import-maa` | 账号管理；`add` 交互式收集 account_name 等 |
| `device` | `add \| list \| show \| remove \| test` | 设备管理 |
| `provision` | `<account> [--device]` | 账号预置引导（§9.3） |
| `switch` | `<account> [--device] [--timeout]` | 手动切号（短暂持有设备租约） |
| `session` | `start <account> [--executor mower\|maa] [--slice 90m]` \| `stop <id\|--account>` \| `list` \| `logs <id> [-f]` | 会话管理（priority=100 插队不抢占） |
| `schedule` | `show \| pause \| resume` | 调度器开关 |
| `maa` | `install \| update [--resource-only] \| version` | 包装 maa-cli |
| `mower` | `update [--ref <git-ref>] [--runner docker\|process] \| version` | mower 更新（含回滚提示） |
| `export` | `[--out FILE] [--redact-secrets] [--stats-only]` | 导出 |
| `import` | `<FILE> [--merge\|--replace]` | 导入 |
| `migrate` | | 手动执行 schema 迁移 |
| `completions` | `<shell>` | shell 补全 |

示例（rails 式体验）：

```bash
arkknight init && arkknight doctor                 # 初始化 + 体检
arkknight device add redroid-main --host-adb 127.0.0.1:2771 --docker-adb arknights:5555
arkknight account add main --server official --account-name '123****8901'
arkknight provision main --device redroid-main # 人工登录一次（投屏指引）
arkknight switch main                          # 验证切号
arkknight session start main --slice 2h        # 手动开会话
arkknight server --open                        # 常驻调度 + 控制台
```

---

## 16. 跨平台注意事项

- 路径全部走 `PathBuf` + `dirs` crate；工作目录与配置内路径支持 `~` 展开；Windows 路径分隔符与 TOML 转义在物化器单测覆盖
- adb：默认 `adb`（PATH），可 `paths.adb_path` 指定；Windows 常见为 platform-tools 绝对路径
- Docker：bollard 在 Linux 走 unix socket、Windows/macOS 走 Docker Desktop（命名管道/TCP）——DockerRunner 三平台可用（装了 Docker Desktop 即可）；**RedroidDocker 设备后端仅 Linux**（GPU/binderfs 依赖），文档明示
- ProcessRunner：需要本地 Python 3.11+（mower alpha 要求）与 mower 依赖；`doctor` 检出并给出安装指引
- 信号处理：Windows 无 SIGTERM —— 统一用 `tokio::process Child::kill` + 优雅 API（mower `/stop`）双通道
- 文件锁：flock(Linux/macOS) 与 LockFile(Windows) 抽象（`fslock` crate 候选）
- 时区：调度计算统一游戏日界（UTC-4 偏移的 04:00 本地表达），展示用 `scheduler.timezone`

---

## 17. 安全考量

- Web 控制台默认 `127.0.0.1`；暴露局域网需显式改 bind + 强制 token（启动时若 bind 非 loopback 且 token 空 → 拒绝启动并告警）
- 凭据落盘现状：mower bundle conf.yml 内含 skland/邮箱凭据（与今日部署一致）；导出默认包含（迁移场景需要），`--redact-secrets` 脱敏，manifest 标记；文档明示风险
- 不引入远端服务；MAA 资源更新 URL 走 maa-cli 自身配置（可代理）
- Docker 套接字：仅 DockerRunner/RedroidDocker 使用 bollard（进程内），控制台不透传 docker.sock（与 mini-ui 挂 sock 的做法不同，降低攻击面）

---

## 18. 可观测性与通知

- 结构化日志（tracing）：`RUST_LOG` 分级；会话事件全量入 `session_events`
- WS 事件流推送状态机迁移，控制台实时可视化
- 通知：M1 仅控制台横幅 + 日志；M3 预留 webhook（Server酱/企业微信/邮件，复用账号 bundle 的 SMTP 配置可选）
- 统计：会话时长/切号次数自动；mower report.csv 定期导入聚合（理智消耗、材料等以 mower 口径为准）

---

## 19. 测试策略

| 层 | 策略 |
|---|---|
| 单元 | 物化器 golden-file 测试（MAA TOML/mower conf patch 快照）；状态机迁移表驱动测试；退避/时间窗纯函数（mock 时钟 `tokio::time::pause`）；端口分配并发测试 |
| 集成 | daemon/standalone 双模式互斥（文件锁）；API 契约测试（OpenAPI schema 校验）；export/import 往返（含 redacted、跨 schema_version 拒绝） |
| 冒烟（对真实环境） | 现有部署（redroid-2771 + mower 检出）上跑 M1 验收脚本：2 账号预置→切号→时间片轮转→深链 UI 编辑持久化→导出导入 |
| 执行器假件 | `FakeExecutor`（trait 的红利）：调度器全场景可在 CI 无设备跑通。设备层假件（`FakeDeviceBackend`）**尚未实现** —— 设备测试目前走 `ExternalBackend` + 不存在的端口断言失败 |

CI（GitHub Actions）：fmt + clippy(deny warnings) + test（三平台矩阵）+ 前端 lint/build。

> 操作手册（golden 快照更新、调度场景假件、避免 flaky 的具体做法）见
> [`dev/testing.md`](./dev/testing.md)。

---

## 20. 里程碑与验收标准

### M1 · 单模拟器（一期）

| # | 交付 | 验收标准 |
|---|---|---|
| 1 | 项目骨架 | workspace + CI 三平台绿；`arkknight init/doctor` 可用，doctor 能发现现有部署全部组件 |
| 2 | 域模型+配置层 | account/device/arkknight.toml 解析与校验；物化器 golden 测试通过 |
| 3 | External 设备 + 租约 | `device add/test` 对 2771 实例连通；租约互斥测试 |
| 4 | MAA 集成 + 切号 | `arkknight switch` 在真实设备完成切号（2 账号互切成功各 ≥3 次）；失败退避生效 |
| 5 | provision 流程 | 引导式预置，`logins` 记录正确 |
| 6 | MowerExecutor（Docker+Process 双 Runner） | 会话起停、端口分配、深链 UI 可编辑且**改动持久化**（双向规则验证） |
| 7 | 调度器 v1 | 单设备时间窗+优先级+时间片轮转+双超时+看门狗；FakeExecutor 全场景 CI 通过 |
| 8 | 控制台 v1 + API | 页面清单全可用；OpenAPI 生成；token 鉴权 |
| 9 | 导入导出 | export/import 往返测试 + `--redact-secrets` |
| 10 | 文档 | user 三平台部署指南 + 配置手册；dev 指南；AGENTS.md 生效 |

**M1 总验收**：在现有部署上，2 个账号按配置自动轮转运行 72 小时无人工干预，控制台可观测全程，导出→新目录导入→继续运行。

### M2 · 多模拟器

RedroidDocker 后端（模板建池/销毁/滚动重建）；水位准入扩容（动态上限）；亲和矩阵调度；模拟器镜像升级流水线（APK+资源+commit 标签）。验收：3 账号 3 设备水位内自动扩缩，游戏版本升级流水线一次完整演练。

### M3 · 加分项

mower 日志事件模式集 v1；空闲信号提前释放；跑单窗口优先锁设备；`cross_account` 开关；通知渠道。验收：A 账号空闲窗口被 B 账号跑单会话复用的演示用例。

---

## 21. 风险登记册

| # | 风险 | 概率/影响 | 对策 |
|---|---|---|---|
| R1 | 游戏登录界面改版致 MAA 切号失效 | 中/高 | INV-1 本意：跟随 MAA 资源热更新即恢复；切号失败即时告警；`maa update` 一键拉新 |
| R2 | mower alpha 分支不稳定破坏会话 | 中/中 | UpdatePlan pin commit + 一键回滚；会话看门狗+退避自愈 |
| R3 | redroid 单实例资源开销大，扩容易触顶 | 高/中 | 水位准入门控 + 排队降级；模板资源上限可调 |
| R4 | 多账号封号风险（同设备多号自动化） | 低/高 | 与现状（手动多开）风险等同，文档明示自担；不提供任何行为伪装（诚实原则） |
| R5 | 官服/B 服混装同设备的包名/资源差异引发切号与启动错乱 | 中/中 | M1 约束：同设备账号同 server 类型；doctor 校验提示；M2 评估双包共存 |
| R6 | Windows 原生路径/Docker Desktop 差异拖慢跨平台 | 中/中 | CI 三平台矩阵从 M1 起即跑；路径处理单测前置 |
| R7 | SQLite/文件双源一致性 | 低/中 | INV-4：文件为事实源，运行态可重建；daemon 单写者 + flock |
| R8 | 端口冲突（mower 会话段/redroid 段被占） | 中/低 | 分配表+绑定前探测；doctor 报告占用 |
| R9 | MAA profile 与 mower conf 的 adb 字段双写不一致 | 中/中 | 物化器单一出口 + 会话启动瞬间改写的时序约束（§11.4） |

---

## 22. 开放问题

| # | 问题 | 处理 |
|---|---|---|
| Q1 | mower 是否有「下次任务时间」REST 端点（M3 提前释放的回切时机） | M3 spike：审读 server.py 路由；无则解析 WS 日志任务生成事件 |
| Q2 | 龙舌兰跑单窗口的可预测性（trigger 为运行时表达式） | M3：以日志事件+保守提前量锁定，不做静态预测 |
| Q3 | B 服同设备双包共存（com.hypergryph.arknights.bilibili） | M2 评估；M1 文档约束同设备同 server |
| Q4 | maa-cli 在 Windows 的 MaaCore Alpha 渠道差异 | doctor 按平台给出渠道建议 |
| Q5 | mower tmp/data.db 按账号长期增长的清理策略 | M1 简单上限（按大小轮转），M2 细化 |

---

## 23. 附录

### 附录 A：调研来源

- mower 主仓库与 issue #340（多账号=多实例建议）：https://github.com/ArkMowers/arknights-mower
- MAA 开始唤醒/账号切换官方文档：https://docs.maa.plus/manual/introduction/startup
- maa-cli 配置文档（profiles/tasks/更新）：https://docs.maa.plus/zh-cn/manual/cli/config
- maa-cli 仓库与示例配置：https://github.com/MaaAssistantArknights/maa-cli
- MAA 主仓库：https://github.com/MaaAssistantArknights/MaaAssistantArknights
- redroid 官方：https://github.com/remote-android/redroid-doc；多实例实践（端口映射/按需启停）见 ivonblog 等
- 本地代码考古：`/srv/arknights`（compose.yml、Dockerfile.mower、entrypoint.sh、watchdog.sh、refill-akdata.sh、redroid-script/、_mower/config/*、arknights-mower 子模块源码）
- AUTO-MAS（设计参考）：https://github.com/AUTO-MAS-Project/AUTO-MAS

### 附录 B：物化产物示例

**MAA profile**（`accounts/<key>/maa/profiles/default.toml`，切号前运行期改写 address）：

```toml
[connection]
address = "127.0.0.1:2771"     # host_adb；DockerRunner 内则物化为容器网络地址
config = "CompatPOSIXShell"     # 平台相关，物化器按 OS 生成
[instance_options]
touch_mode = "ADB"
[resource]
# 官服默认；B 服账号物化为对应 platform 差异
```

**MAA startup 任务**（`accounts/<key>/maa/tasks/startup.toml`）：

```toml
[[tasks]]
name = "切号并启动"
type = "StartUp"
params = { client_type = "Official", account_name = "123****8901", start_game_enabled = true }
```

**mower conf patch**（会话启动瞬间白名单改写，serde_yaml 精确 patch）：

```yaml
adb: arknights:5555            # 或 127.0.0.1:2771（按 Runner）
start_automatically: true
webview:
  port: 58100                  # 会话分配
  token: <会话token>            # 控制台深链拼接用
```

### 附录 C：设备注册示例（`devices/redroid-main.toml`）

```toml
name = "redroid-main"
backend = "external"

[connection]
host_adb = "127.0.0.1:2771"
docker_adb = "arknights:5555"
docker_network = "arknights_default"

notes = "现有单实例（arknights2771），nvidia GPU，watchdog 已注入"
```

### 附录 D：一次典型时序（M1，单设备双账号）

```
08:00  账号A时间窗开启，调度器发起会话S1
       S1: Queued→获租约→Switching(maa startup, ~90s)→Running(mower, port 58100)
10:00  S1 slice 到期→Draining(mower /stop, grace 120s)→Finished
       队列头账号B（同窗等待中）→S2: Switching→Running
12:00  S2 slice 到期→Draining→Finished→S3(账号A)……
14:30  S3 看门狗连续失败→Draining→TimedOut→账号A进入退避(5m)→重排账号B
23:50  时间窗结束，设备空闲
```
