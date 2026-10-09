# arkreunion 文档中心

方舟多账号调度中心（arkreunion）—— 跨平台编排 MAA 与 mower 的多账号自动化调度器。

| 受众 | 入口 | 内容 |
|---|---|---|
| **总设计（权威）** | [`arkreunion-design.md`](./arkreunion-design.md) | 架构、域模型、调度、配置、API、里程碑、风险 —— 一切行为变更的先行文档 |
| AI 协作者 / 新开发者 | [`ai/AGENTS.md`](./ai/AGENTS.md) | 代码地图、硬性不变量、开发约定、常见坑 |
| 架构决策 | [`ai/adr/`](./ai/adr/) | ADR 记录（[0001 技术选型与架构基线](./ai/adr/0001-技术选型与架构基线.md)） |
| 开发者 | [`dev/README.md`](./dev/README.md) | 构建、测试、API、发布 |
| 最终用户 | [`user/README.md`](./user/README.md) | 安装、快速上手、配置手册、FAQ |

## 文档规则

1. **文档先行**：行为变更先改 `arkreunion-design.md`（或新写 ADR），再改代码。
2. **三同步**：新增/修改配置字段时，同步更新设计文档 §11、用户配置手册、golden 测试。
3. **单一权威**：同一事实只在一处定义（设计文档），其余引用不复制。
