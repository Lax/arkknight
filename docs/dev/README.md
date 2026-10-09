# arkreunion 开发者文档

面向为本项目贡献代码的人类开发者与 AI 协作者（AI 协作者请先读 [`../ai/AGENTS.md`](../ai/AGENTS.md)）。

## 必读

1. [总体设计 `arkreunion-design.md`](../arkreunion-design.md) —— 唯一权威设计：架构、域模型、状态机、API、里程碑
2. [ADR 目录 `../ai/adr/`](../ai/adr/) —— 架构决策及其理由；新决策先写 ADR（模板见下）
3. [`../ai/AGENTS.md`](../ai/AGENTS.md) —— 代码地图、命名约定、硬性不变量、修改热点、常见坑

## 快速上手

```bash
# 后端（Rust workspace：crates/arkreunion-core + crates/arkreunion）
cargo build --release               # 发布构建（UI 在此阶段嵌入）
cargo test --workspace              # 全量：单测 + golden + 调度场景 + server 测试
cargo clippy --workspace --all-targets -- -D warnings   # 零告警是硬约定
cargo fmt
cargo run -p arkreunion -- init --dir /tmp/arkreunion-demo   # 体验 CLI

# 前端（ui/，构建产物经 rust-embed 嵌入二进制）
cd ui && npm ci && npm run build   # vue-tsc 类型检查 + vite build
```

> ⚠️ **release 构建必须先有 `ui/dist`**，否则 `rust-embed` 编译期嵌入失败。
> 这是特性：保证发布物一定带最新控制台。改前端后记得 `npm run build`。

冒烟（对真实环境）：`arkreunion init` → `arkreunion device add <name> --host-adb <addr>` →
`arkreunion device test <name>` → `arkreunion account add <key> --server official --account-name '138****0000' --uid <uid>` →
`arkreunion provision <key>` → `arkreunion switch <key>` → `arkreunion server --open`。

## 文档

| 文档 | 内容 |
|---|---|
| [`api.md`](./api.md) | **实际已实现**的 REST/WS 路由、鉴权、错误约定、契约测试 |
| [`testing.md`](./testing.md) | 测试分层、golden 快照更新、调度场景假件、避免 flaky |
| [`release.md`](./release.md) | 三平台发布流程、产物矩阵、Gentoo overlay |
| [`device-agent-research.md`](./device-agent-research.md) | Android 设备侧 Agent 方案调研 |
| [`redroid-vs-waydroid.md`](./redroid-vs-waydroid.md) | 模拟器后端选型对比 |
| `architecture.md` | 模块深读（调度器/物化器/Runner 细节）—— **待补**；现阶段以设计文档 + AGENTS.md 为准 |

### 「已实现」与「草案」的区分

设计文档 §13.2 的 REST API 是**含未实现项的草案**。实际有什么看
[`api.md`](./api.md) 的路由表与「未实现」段。文档里凡是未实现的都显式标注，
避免照着草案集成后扑空。

## 命名约定（易错）

`Account` 的四个标识字段语义各不相干：

| 字段 | 归属 | 含义 |
|---|---|---|
| `key` | **本项目内部** | 本地定位键（目录 `accounts/<key>/`、CLI 位置参数、API 字段） |
| `display_name` | 内部 | 展示名 |
| `account_name` | 游戏侧 | MAA 切号匹配串 |
| `uid` | 游戏侧 | 游戏 UID，切号后 OCR 核验 |

`key` **不表达任何游戏身份**。理由见 [ADR-0004](../ai/adr/0004-账号本地标识用key.md)。

改字段名/配置项时要同步全链路：模型 → store（含 schema 迁移）→ CLI → API → 前端
→ 设计文档 → [`../user/config.md`](../user/config.md)。

## 本机实机验证栈

`/srv/reunion` —— docker compose 部署（redroid 2781 + arkreunion + mower），
配置独立且 git 管理（明文凭据拆分后不入库）。用它做真机验证，
**不要污染 `/srv/arknights` 既有栈**。

```bash
cd /srv/reunion
docker compose ps
docker compose logs -f arkreunion

ark() { docker compose run --rm --workdir /arkreunion \
        --entrypoint /usr/local/bin/arkreunion arkreunion "$@"; }
ark doctor
ark status
```

细节见 `/srv/reunion/README.md`。

## ADR 模板

```markdown
# ADR-XXXX：<标题>
- **状态**：提案 | 已接受 | 已废弃（被 ADR-YYYY 取代）
- **日期**：YYYY-MM-DD
- **关联**：（设计文档章节 / 相关 ADR）

## 背景
（为什么需要这个决策 —— 具体到踩了什么坑或发现了什么矛盾）

## 决策
（选了什么，一句话讲清）

## 备选与否决理由
（每个候选 + 为什么否决。这一段是 ADR 的价值所在，别省）

## 后果（✅收益 / ⚠️代价）
```

## 约定速查

- **提交前门禁**：`cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace` + `cd ui && npm run build`
- Conventional Commits；**一个 PR 一件事**；改行为先改 `arkreunion-design.md`
- 新配置字段三同步：设计文档 §11 schema、[`user/config.md`](../user/config.md)、`golden` 快照
- 严禁登录界面自研自动化（INV-1，见 AGENTS.md）
- 错误响应一律带 `hint`（见 [ADR-0006](../ai/adr/0006-错误响应带hint.md)）
- 测试里不写真实凭据；端口相关测试运行时探测空闲端口，不硬编码