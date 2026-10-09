# AGENTS.md — arkreunion AI 协作规范

> 本文档面向在 arkreunion 仓库工作的 AI 编程助手（以及新加入的人类开发者）。
> 人类开发者请同时阅读 `docs/dev/README.md`；最终用户文档在 `docs/user/`。

## 项目是什么

arkreunion 是一个跨平台（Linux/Windows/macOS）的明日方舟**多账号编排器**：编排 MAA（账号切换/任务）与 mower（基建排班）等执行器，在少量安卓设备（redroid/原生模拟器）上自动轮转运行多个游戏账号。**它自己不做任何游戏内图像识别与操作自动化。**

- 总体设计（必读）：[`docs/arkreunion-design.md`](../arkreunion-design.md)
- 架构决策与理由：[`adr/`](./adr/)（改动架构前先读相关 ADR；新决策先写 ADR）

## 代码地图（随实现更新）

```
crates/arkreunion-core/      库：领域模型/配置/物化/调度器/执行器/设备后端/持久化
  src/model/            Account, Device, Session, SchedulePolicy（见设计文档 §6）
  src/config/           arkreunion.toml、工作目录布局、export/import
  src/materialize/      MAA TOML 与 mower conf.yml 的物化与白名单 patch（§11）
  src/device/           DeviceBackend trait + external/ + redroid/(M2)
  src/executor/         Executor trait + maa/ + mower/{docker,process} runners
  src/scheduler/        状态机、队列、看门狗、退避（§10）
  src/switch/           账号切换的唯一实现入口（§9，受 INV-1 约束）
  src/store/            SQLite(rusqlite, WAL) + schema 迁移
crates/arkreunion/           CLI(clap) + axum server + rust-embed 前端
ui/                     Vue3 + Vite + Naive UI 控制台源码
docs/                   本文档树
```

**当前状态**（2026-10-09，M1 任务 1-5 完成、任务 6 前半完成）：
- 已实现：workspace 骨架、域模型与配置层、环境探测 + doctor、DeviceBackend/Executor trait + External 后端、
  物化器（MAA/mower，golden 测试）、CLI init/doctor/status/account/device/completions（任务 1/2）；
  **store 层**（rusqlite WAL + schema v1 迁移 + 租约/端口记账/logins + 单写者 flock，任务 3）；
  **MaaCliExecutor + switch::run_switch**（INV-1 唯一路径：force-stop→物化→`maa run startup --batch`→
  重试退避→switch_log；maa 路径可注入，fake 测试覆盖；`arkreunion maa install|update|version`；任务 4）；
  **provision**（投屏指引 + 回车确认 + logins 记录 + 租约互斥，任务 5）；
  **MowerProcessExecutor + session start/stop/list/logs**（端口分配 + conf 白名单 patch + 深链 +
  日志归档 logs/sessions/<id>.log + POST /stop 优雅停止→pid 兜底强杀；任务 6 前半）
- **调度器 v1（任务 7，2026-10-09）**：scheduler/{engine,session,window}——游戏日界时间窗纯函数、
  就绪判定/队列（priority desc + FIFO）、指数退避（内存态）、双超时+看门狗（MonitorParams select 环）、
  崩溃恢复（启动全清租约+标记中断会话）、会话独立 task + 引擎 stop 注入（防死锁）、
  `schedule pause/resume` 经 daemon API、SIGINT/SIGTERM 优雅关停；FakeExecutor 五场景测试
- **控制台 v1（任务 8 大半，2026-10-09）**：API 完整化（accounts CRUD/devices test/doctor/
  session logs/手动会话 POST /api/sessions——活跃占用 409、暂停保护、结束还原）+
  WS（`?logs=<id>` 日志 tail、`?events=1` 事件流 EventBus）+ rust-embed 嵌入 `ui/dist`
  （debug 读盘、未构建回退指引页）+ 前端七页（总览/会话+日志/日志实时/账号 CRUD/
  设备测试/调度只读/维护 doctor）；引擎重构出 EngineShared（server 与引擎共享依赖）
- **UID 核验（§9.4，2026-10-09）**：切号成功后经 MAA 自定义 pipeline（--user-resource +
  OCR 个人信息页期望 UID）核验登录身份，失败=SwitchFailed 防串数据；`Account.uid` 经
  provision/add/PATCH 录入，doctor 对缺 uid 告警（MAA 切号静默失败系 issue #15309）
- **发布自动化（2026-10-09）**：`.github/workflows/release.yml`——tag `v*` 自动构建
  6 目标安装包（linux x86_64/aarch64 + windows x86_64 + macos aarch64/Intel；aarch64 交叉编译走
  CC/LINKER 环境变量，rusqlite bundled C 一并交叉）+ deb + Arch Linux 包（容器内 makepkg 重打包）+ Gentoo -bin ebuild（外部 overlay 脚手架，
  thin-manifests）+ SHA256SUMS + 自动 GitHub Release；
  ui job 先行构建（rust-embed release 编译期嵌入，缺 dist 会编译失败属特性）；
  cargo-deb 配置在 crates/arkreunion/Cargo.toml；流程文档 docs/dev/release.md
- 进行中/未做：任务 6 后半（**DockerRunner（bollard）** + mower update UpdatePlan）→
  任务 8 尾批（OpenAPI/utoipa、统计页 ECharts、调度策略表单编辑）→ 任务 9（导入导出）
  → 72h 真机验收
- 实现备注：真实环境切号验证依赖 provision 后的账号（需人工登录），当前以 fake maa/adb 冒烟 +
  真机 device test/doctor 覆盖；`maa run --dry-run` 可用于无设备校验物化配置
- 测试：`cargo test`（59 单测 + 4 golden + 5 调度场景）；前端 `cd ui && npm run build`（含 vue-tsc）；golden 更新：`ARKREUNION_UPDATE_GOLDEN=1 cargo test -p arkreunion-core --test golden`

## 硬性不变量（违反即拒改）

1. **INV-1 账号切换只许调 MAA**：唯一路径 `switch::run_switch()` → `maa run startup -p default --batch`。**严禁**编写任何登录界面截图识别、坐标点击、OCR、输入模拟代码。MAA 不可用→任务失败告警，不降级。
2. **INV-2 跨平台与可迁移**：不引入平台专属依赖（除非 feature-gated 且默认关）；所有状态必须可 export/import。
3. **INV-3 执行器/设备可插拔**：调度器只面向 trait 编程；新执行器不改调度器核心。
4. **INV-4 单一事实源**：账号/设备/策略以工作目录文件树为准；SQLite 只存运行态与统计；物化产物可重建（mower bundle 的用户改动除外，见设计文档 §11.4）。

## 开发约定

- **文档先行**：改行为先改设计文档对应章节（或 ADR），再写代码；文档与代码不一致视为 bug。
- Rust：`cargo fmt` + `cargo clippy -- -D warnings` 零告警；公共 API 写 doc comment 与示例。
- 提交信息：Conventional Commits（`feat(scheduler): ...`）；一个 PR 一件事。
- 新增配置字段：同步更新 `arkreunion-design.md` §11 schema、`user/` 配置手册、golden 测试快照。
- 涉及密钥的测试/夹具一律用假值；永不把真实 `account_name`/凭据写进仓库。
- 测试：调度器与执行器场景用 `FakeExecutor`/`FakeDeviceBackend` 在 CI 跑；物化器必须配 golden-file 快照。

## 常见坑（来自本地部署考古）

- mower 的 `conf.yml` 顶层 `account` 字段是 **SMTP 邮箱用户名**，不是游戏账号；游戏账号信息在 skland_info 与 MAA 侧。
- adb 地址有**双形态**：宿主端口（`127.0.0.1:2771`）与容器网络（`arknights:5555`）。物化时按 Runner 形态选择，写错是高频 bug。
- mower 数据根由 `MOWER_DATA_DIR` 决定（ProcessRunner 依赖）；DockerRunner 则直接挂载 `/mower/config`。
- 游戏日界是官服 UTC-4 的 04:00，不要按本地零点写时间窗逻辑。
- mower alpha 分支会动配置 schema（如 device 段迁移），更新 mower 后先跑 `doctor`。

## AI 协作流程建议

1. 动手前：读 `arkreunion-design.md` 对应章节 + 相关 ADR + 邻近模块代码。
2. 不确定的设计点：查 `arkreunion-design.md` §22 开放问题；仍未覆盖则停下提问，不要自行发明架构。
3. 交付时：自述改了哪些模块、测试结果、文档是否同步。
