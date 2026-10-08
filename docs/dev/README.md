# akops 开发者文档

面向为本项目贡献代码的人类开发者与 AI 协作者（AI 协作者请先读 [`../ai/AGENTS.md`](../ai/AGENTS.md)）。

## 必读

1. [总体设计 `akops-design.md`](../akops-design.md) —— 唯一权威设计：架构、域模型、状态机、API、里程碑
2. [ADR 目录 `../ai/adr/`](../ai/adr/) —— 架构决策及其理由；新决策先写 ADR（模板见下）
3. [`../ai/AGENTS.md`](../ai/AGENTS.md) —— 代码地图、硬性不变量、常见坑

## 快速上手（已可用）

```bash
# 后端（Rust workspace：crates/akops-core + crates/akops）
cargo build                       # 调试构建
cargo test                        # 单测 + golden 快照比对
AKOPS_UPDATE_GOLDEN=1 cargo test -p akops-core --test golden   # 更新物化器快照
cargo clippy --workspace --all-targets -- -D warnings          # 零告警约定
cargo run -p akops -- init --dir /tmp/akops-demo               # 体验 CLI

# 前端（ui/，构建产物未来经 rust-embed 嵌入二进制）
cd ui && npm install && npm run build
```

冒烟（对真实环境）：`akops init` → `akops device add <name> --host-adb <addr>` →
`akops account add <id> --server official --account-name '138****0000'` →
`akops device test <name>` → `akops doctor`。

## 文档规划（随代码落地逐步填充）

| 文档 | 内容 | 状态 |
|---|---|---|
| `architecture.md` | 模块深读（调度器/物化器/Runner 细节），链接设计文档对应章节 | 占位 |
| `api.md` | REST/WS API 参考；以 `/docs` 的 OpenAPI 为准，此处写设计意图与版本策略 | 占位（任务 8） |
| `testing.md` | 测试策略（FakeExecutor/FakeDeviceBackend、golden-file、冒烟脚本） | 设计见总设计 §19 |
| `release.md` | 三平台发布流程（crates.io / GitHub Releases / Docker） | 占位 |

## ADR 模板

```markdown
# ADR-XXXX：<标题>
- 状态：提案 | 已接受 | 已废弃（被 ADR-YYYY 取代）
- 日期：YYYY-MM-DD
## 背景
## 决策
## 备选与否决理由
## 后果（✅收益 / ⚠️代价）
```

## 约定速查

- `cargo fmt` + `cargo clippy -- -D warnings`；前端 `npm run build`（含 vue-tsc 类型检查）
- Conventional Commits；改行为先改 `akops-design.md`
- 新配置字段三同步：设计文档 §11 schema、`user/` 配置手册、golden 测试
- 严禁登录界面自研自动化（INV-1，见 AGENTS.md）
