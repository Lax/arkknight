# akops 用户文档

面向部署与使用 akops 的最终用户。

## akops 是什么

一个跨平台（Linux / Windows / macOS）的明日方舟多账号调度中心：在一台（或按需多台）安卓模拟器设备上自动切换并轮转运行多个游戏账号，账号切换由 MAA 官方能力完成，基建排班由 mower 完成。

> ⚠️ 使用自动化工具的风险由用户自担；请勿用于代练等商业用途。

## 文档规划（随版本发布逐步填充）

| 文档 | 内容 | 状态 |
|---|---|---|
| `install.md` | 三平台安装（cargo install / Releases / Docker）与依赖（adb、MAA、mower） | 占位 |
| `quickstart.md` | 从零到跑通：init → doctor → 设备 → 账号 → 人工登录一次 → 切号 → 会话 → server | 占位 |
| `config.md` | 配置手册：akops.toml / 账号 / 设备 / 调度策略全字段说明 | 以 [`../akops-design.md` §11](../akops-design.md) 为准 |
| `console.md` | Web 控制台使用（含深链 mower UI 编辑配置） | 占位 |
| `backup-migrate.md` | 导出/导入迁移（含敏感信息脱敏） | 占位 |
| `faq.md` | 常见问题：切号失败、adb 连不上、端口冲突、游戏更新后怎么办 | 占位 |

## 核心概念速览

- **设备**：一台可 adb 连接的安卓环境（redroid 容器 / MuMu / 雷电 / 物理机）
- **账号**：一个游戏账号 + 它的 MAA/mower 私有配置（bundle）
- **切号**：由 MAA「开始唤醒」在登录界面自动完成（账号需在该设备登录过一次）
- **会话**：一次「占用设备 → 切号 → 运行 mower/MAA → 释放」的完整过程；多账号按时间片轮转

## 最短路径（预览，正式版见 quickstart）

```bash
akops init && akops doctor
akops device add redroid-main --host-adb 127.0.0.1:2771
akops account add main --server official --account-name '123****8901'
akops provision main --device redroid-main   # 按提示人工登录一次
akops server --open                           # 常驻调度 + 打开控制台
```
