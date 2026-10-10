# arkknight 用户文档

面向部署与使用 arkknight 的最终用户。

## arkknight 是什么

一个跨平台（Linux / Windows / macOS）的明日方舟多账号调度中心：在一台（或按需多台）安卓模拟器设备上自动切换并轮转运行多个游戏账号，账号切换由 MAA 官方能力完成，基建排班由 mower 完成。

> ⚠️ 使用自动化工具的风险由用户自担；请勿用于代练等商业用途。账号共享/自动化可能违反游戏用户协议。

## 文档

| 文档 | 内容 |
|---|---|
| [`install.md`](./install.md) | 依赖（adb / MAA / mower / Python）、三种安装方式、Docker 部署要点 |
| [`quickstart.md`](./quickstart.md) | 从零跑通：init → doctor → 设备 → 账号 → 人工登录 → 切号 → 会话 → server |
| [`deploy.md`](./deploy.md) | 生产部署：systemd / 容器化 / socket-proxy、redroid 设备镜像升级、备份与升级 |
| [`config.md`](./config.md) | `arkknight.toml` / 账号 / 设备全字段、四个标识字段的分工、环境变量 |
| [`console.md`](./console.md) | Web 控制台七页、token 设置、截图、错误提示怎么读 |
| [`faq.md`](./faq.md) | 切号失败、adb 连不上、白屏、端口冲突、调度不启动、游戏更新 |

配置字段的**权威定义**在 [`../arkknight-design.md` §11](../arkknight-design.md)；
本文补充面向使用者的解读与陷阱说明。

## 核心概念

| 概念 | 是什么 |
|---|---|
| **设备（device）** | 一台可 adb 连接的安卓环境（redroid 容器 / MuMu / 雷电 / 物理机）。M1 只连不管生命周期 |
| **账号（account）** | 一个游戏账号 + 它的 MAA/mower 私有配置 bundle |
| **账号 key** | 账号的**本地定位键**（= 目录名）。不是游戏身份 —— 游戏身份是 `account_name` 与 `uid` |
| **切号（switch）** | 由 MAA「开始唤醒」在登录界面自动完成。账号需在该设备登录过一次 |
| **预置（provision）** | 每账号每设备一次性的人工登录引导，之后才能自动切号 |
| **会话（session）** | 「占用设备 → 切号 → 运行 mower/MAA → 释放」的完整过程。多账号按时间片轮转 |
| **工作目录（workdir）** | 所有状态所在：`arkknight.toml` + `devices/` + `accounts/` + `state/` + `logs/` |

## 四个标识字段别混

这是最容易搞错的地方：

| 字段 | 归属 | 用途 |
|---|---|---|
| `key` | **本项目内部** | 目录名 + CLI 参数。如 `main` |
| `display_name` | 内部 | 仅展示 |
| `account_name` | 游戏侧 | MAA 切号匹配串（官服=打码手机号片段，B服=昵称） |
| `uid` | 游戏侧 | 游戏 UID，切号后 OCR 核验身份，**防登错号串数据** |

`key` 不表达任何游戏身份；改它等于换目录。

## 最短路径

```bash
arkknight init --dir ~/arkknight && cd ~/arkknight
arkknight doctor
arkknight device add redroid-main --host-adb 127.0.0.1:2771
arkknight device test redroid-main
arkknight account add main --server official --account-name '123****8901' --uid 1000123456
arkknight provision main        # 人工登录一次（切号前提）
arkknight switch main           # 验证
arkknight server --open         # 常驻调度 + 控制台
```

完整版见 [`quickstart.md`](./quickstart.md)。