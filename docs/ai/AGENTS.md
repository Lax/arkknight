# AGENTS.md — akops AI 协作规范

> 本文档面向在 akops 仓库工作的 AI 编程助手（以及新加入的人类开发者）。
> 人类开发者请同时阅读 `docs/dev/README.md`；最终用户文档在 `docs/user/`。

## 项目是什么

akops 是一个跨平台（Linux/Windows/macOS）的明日方舟**多账号编排器**：编排 MAA（账号切换/任务）与 mower（基建排班）等执行器，在少量安卓设备（redroid/原生模拟器）上自动轮转运行多个游戏账号。**它自己不做任何游戏内图像识别与操作自动化。**

- 总体设计（必读）：[`docs/akops-design.md`](../akops-design.md)
- 架构决策与理由：[`adr/`](./adr/)（改动架构前先读相关 ADR；新决策先写 ADR）

## 代码地图（随实现更新）

```
crates/akops-core/      库：领域模型/配置/物化/调度器/执行器/设备后端/持久化
  src/model/            Account, Device, Session, SchedulePolicy（见设计文档 §6）
  src/config/           akops.toml、工作目录布局、export/import
  src/materialize/      MAA TOML 与 mower conf.yml 的物化与白名单 patch（§11）
  src/device/           DeviceBackend trait + external/ + redroid/(M2)
  src/executor/         Executor trait + maa/ + mower/{docker,process} runners
  src/scheduler/        状态机、队列、看门狗、退避（§10）
  src/switch/           账号切换的唯一实现入口（§9，受 INV-1 约束）
  src/store/            SQLite(rusqlite, WAL) + schema 迁移
crates/akops/           CLI(clap) + axum server + rust-embed 前端
ui/                     Vue3 + Vite + Naive UI 控制台源码
docs/                   本文档树
```

**当前状态**（2026-10-08，M1 任务 1/2 已落地）：
- 已实现：workspace 骨架、域模型（Account/Device/Session 词汇）、akops.toml 配置层与工作目录布局、
  环境探测、doctor（验收：能发现现有部署全部组件）、DeviceBackend trait + External 后端（health/游戏包检测）、
  Executor trait、物化器（MAA profile/tasks + mower conf 白名单 patch，golden 测试）、
  CLI：`init/doctor/status/account add|list|show|remove|enable|disable/device add|list|show|remove|test/completions`；
  其余子命令为里程碑占位（报进度提示，不误导）
- 进行中：M1 任务 3（SQLite 租约）→ 任务 4（MAA 切号）→ 任务 5（provision）→ 任务 6（mower 双 Runner）→
  任务 7（调度器）→ 任务 8（server+控制台）→ 任务 9（导入导出）
- 测试：`cargo test`（34 单测 + 4 golden）；golden 更新：`AKOPS_UPDATE_GOLDEN=1 cargo test -p akops-core --test golden`

## 硬性不变量（违反即拒改）

1. **INV-1 账号切换只许调 MAA**：唯一路径 `switch::run_switch()` → `maa run startup -p default --batch`。**严禁**编写任何登录界面截图识别、坐标点击、OCR、输入模拟代码。MAA 不可用→任务失败告警，不降级。
2. **INV-2 跨平台与可迁移**：不引入平台专属依赖（除非 feature-gated 且默认关）；所有状态必须可 export/import。
3. **INV-3 执行器/设备可插拔**：调度器只面向 trait 编程；新执行器不改调度器核心。
4. **INV-4 单一事实源**：账号/设备/策略以工作目录文件树为准；SQLite 只存运行态与统计；物化产物可重建（mower bundle 的用户改动除外，见设计文档 §11.4）。

## 开发约定

- **文档先行**：改行为先改设计文档对应章节（或 ADR），再写代码；文档与代码不一致视为 bug。
- Rust：`cargo fmt` + `cargo clippy -- -D warnings` 零告警；公共 API 写 doc comment 与示例。
- 提交信息：Conventional Commits（`feat(scheduler): ...`）；一个 PR 一件事。
- 新增配置字段：同步更新 `akops-design.md` §11 schema、`user/` 配置手册、golden 测试快照。
- 涉及密钥的测试/夹具一律用假值；永不把真实 `account_name`/凭据写进仓库。
- 测试：调度器与执行器场景用 `FakeExecutor`/`FakeDeviceBackend` 在 CI 跑；物化器必须配 golden-file 快照。

## 常见坑（来自本地部署考古）

- mower 的 `conf.yml` 顶层 `account` 字段是 **SMTP 邮箱用户名**，不是游戏账号；游戏账号信息在 skland_info 与 MAA 侧。
- adb 地址有**双形态**：宿主端口（`127.0.0.1:2771`）与容器网络（`arknights:5555`）。物化时按 Runner 形态选择，写错是高频 bug。
- mower 数据根由 `MOWER_DATA_DIR` 决定（ProcessRunner 依赖）；DockerRunner 则直接挂载 `/mower/config`。
- 游戏日界是官服 UTC-4 的 04:00，不要按本地零点写时间窗逻辑。
- mower alpha 分支会动配置 schema（如 device 段迁移），更新 mower 后先跑 `doctor`。

## AI 协作流程建议

1. 动手前：读 `akops-design.md` 对应章节 + 相关 ADR + 邻近模块代码。
2. 不确定的设计点：查 `akops-design.md` §22 开放问题；仍未覆盖则停下提问，不要自行发明架构。
3. 交付时：自述改了哪些模块、测试结果、文档是否同步。
