# arkknight · 方舟骑士

<div align="center">

**Ark Knight —— Arknights 的谐音，也是字面意思：替博士驻守每个账号夜班的骑士。**

MAA × mower × 多账号 —— 自动上号、基建挂机、到点换人，一个二进制全管。

[安装包](#安装) · [快速开始](#快速开始) · [文档](docs/)

</div>

---

**arkknight** 是一个跨平台的明日方舟**多账号编排器**：
在少量安卓设备（redroid / MuMu / 雷电 / 物理机）上，按你设定的时间窗自动轮转多个游戏账号——
自动上号（MAA 官方切号 + UID 核验防串号）→ 基建挂机（mower）→ 到点下号换人。

它自身不做任何游戏内图像识别与操作；账号切换 100% 调用 MAA 官方能力。
命名致敬《杀手机器人日记》：一个安静值守、自己安排排班的机器人单位。

## 它能做什么

| | |
|---|---|
| 🔄 **多账号轮转** | 时间窗 + 优先级 + 时间片调度；双超时（时间片/硬上限）兜底 |
| 🛡️ **切号防串号** | MAA 官方 `account_name` 切号，切号后 OCR 核验 UID——不对不上号 |
| 🏭 **基建挂机** | mower 会话化：Docker / 本地进程双 Runner，Web UI 深链直达，改动实时持久化 |
| 🖥️ **内嵌控制台** | 单二进制自带 Web 控制台：总览 / 会话 / 日志实时 / 账号 / 设备 / doctor 体检 |
| 📦 **六平台分发** | Linux x86_64+ARM64（tar.gz / deb / Arch）、Windows、macOS Intel+Apple Silicon、Gentoo overlay |

调度器自带看门狗（执行器失联自动收尾）与指数退避（失败不空转）；所有状态迁移入库可追溯。

## 快速开始

```bash
# 1) 安装：从 Release 下载对应平台包解压，或源码安装
cargo install --git https://github.com/Lax/arkknight

# 2) 初始化工作目录（自动探测 adb / maa / mower / docker）
arkknight init --dir ~/arkknight

# 3) 注册设备与账号
arkknight device add redroid-main --host-adb 127.0.0.1:2771 --docker-adb arknights:5555
arkknight account add main --server official --account-name '123****8901' --uid '123456789'
arkknight provision main --device redroid-main   # 人工登录一次（切号前提）

# 4) 验证切号，然后交给调度器
arkknight switch main
arkknight server                                 # daemon + 控制台 http://127.0.0.1:7100
```

之后调度器按时间窗自动轮转：到点切号 → 跑基建 → 时间片到期优雅下号 → 下一个账号。

## 文档

| 文档 | 内容 |
|---|---|
| [总设计](docs/arkknight-design.md) | 架构 / 域模型 / 调度器 / 发布物（权威参考） |
| [发布流程](docs/dev/release.md) | 六平台安装包流水线 |
| [AI 协作规范](docs/ai/AGENTS.md) | 硬性不变量（INV-1~4）与代码地图 |

## 风险须知

自动化工具请自行确认符合游戏用户协议；多账号行为的风险由使用者自担。
本项目遵守诚实原则：**不提供任何行为伪装**，也不内置任何游戏内图像识别与操作自动化（切号与任务均由 MAA 官方能力完成）。

## License

MIT
