# arkknight 文档中心

方舟多账号调度中心（arkknight）—— 跨平台编排 MAA 与 mower 的多账号自动化调度器。

## 按受众

| 受众 | 入口 |
|---|---|
| **总设计（权威）** | [`arkknight-design.md`](./arkknight-design.md) — 架构、域模型、调度、配置、API、里程碑、风险。**一切行为变更的先行文档** |
| AI 协作者 / 新开发者 | [`ai/AGENTS.md`](./ai/AGENTS.md) — 代码地图、命名约定、硬性不变量、修改热点、常见坑 |
| 最终用户 | [`user/`](./user/README.md) — 安装 → 快速上手 → 配置 → 控制台 → FAQ |

## 用户文档

| 文档 | 内容 |
|---|---|
| [`user/install.md`](./user/install.md) | 依赖（adb / MAA / mower / Python）、三种安装方式、Docker 部署要点 |
| [`user/quickstart.md`](./user/quickstart.md) | 从零跑通：init → doctor → 设备 → 账号 → 人工登录 → 切号 → 会话 → server |
| [`user/config.md`](./user/config.md) | `arkknight.toml` / 账号 / 设备全字段，四个标识字段的分工 |
| [`user/console.md`](./user/console.md) | Web 控制台七页、token 设置、截图、错误提示怎么读 |
| [`user/faq.md`](./user/faq.md) | 切号失败、adb 连不上、白屏、端口冲突、调度不启动、游戏更新 |

## 开发者文档

| 文档 | 内容 |
|---|---|
| [`dev/README.md`](./dev/README.md) | 快速上手、约定速查、ADR 模板 |
| [`dev/api.md`](./dev/api.md) | **实际已实现**的 REST/WS 路由、鉴权、错误约定、契约测试 |
| [`dev/testing.md`](./dev/testing.md) | 测试分层、golden 快照更新、调度场景假件、避免 flaky |
| [`dev/release.md`](./dev/release.md) | 三平台发布流程、产物矩阵、Gentoo overlay |
| [`dev/device-agent-research.md`](./dev/device-agent-research.md) | Android 设备侧 Agent 方案调研 |
| [`dev/redroid-vs-waydroid.md`](./dev/redroid-vs-waydroid.md) | 模拟器后端选型对比 |

## 架构决策记录（ADR）

| ADR | 主题 |
|---|---|
| [0001](./ai/adr/0001-技术选型与架构基线.md) | 语言、前端、依赖、目录布局等立项基线 |
| [0002](./ai/adr/0002-物化器YAML白名单改写的保真权衡.md) | 物化器改写 mower conf 的保真边界 |
| [0003](./ai/adr/0003-模拟器池化后端维持redroid.md) | 池化后端为何维持 redroid（否决 waydroid） |
| [0004](./ai/adr/0004-账号本地标识用key.md) | `Account.id` → `key`（BREAKING） |
| [0005](./ai/adr/0005-Web控制台鉴权只作用于API.md) | 鉴权只拦 `/api/*`，修控制台白屏 |
| [0006](./ai/adr/0006-错误响应带hint.md) | 错误响应带 `hint`，面向「去哪里改」 |

## 文档规则

1. **文档先行**：行为变更先改 `arkknight-design.md`（或新写 ADR），再改代码。
2. **三同步**：新增/修改配置字段时，同步更新设计文档 §11、
   [`user/config.md`](./user/config.md)、golden 测试快照。
3. **单一权威**：同一事实只在一处定义（设计文档），其余引用不复制。
   - `user/config.md` 补充使用者视角的解读与陷阱，不重复 schema 定义
   - `dev/api.md` 记录**实际实现**的路由（设计文档 §13.2 是含未实现项的草案）
4. **区分「已实现」与「草案」**：文档里凡是未实现的，必须显式标注，
   避免读者照着草案集成后扑空（见 `dev/api.md` 的「未实现」段）。