# akops —— 方舟多账号调度中心

跨平台（Linux / Windows / macOS）的明日方舟**多账号编排器**：在少量安卓设备（redroid、MuMu、雷电、物理机等）上，编排 [MAA](https://github.com/MaaAssistantArknights/MaaAssistantArknights)（账号切换 / 任务）与 [arknights-mower](https://github.com/ArkMowers/arknights-mower)（基建排班），按时间窗自动轮转运行多个游戏账号。

**akops 自身不做任何游戏内图像识别与操作自动化**——账号切换 100% 调用 MAA 官方能力（INV-1）。

## 当前状态

🚧 开发中（M1 里程碑）。设计与决策文档见 [`docs/`](docs/README.md)。

## 快速上手（开发预览）

```bash
cargo install --path crates/akops   # 或 cargo run -p akops -- ...

akops init --dir ~/akops            # 初始化工作目录（自动探测 adb/maa/mower/docker）
akops device add redroid-main --host-adb 127.0.0.1:2771 --docker-adb arknights:5555
akops account add main --server official --account-name '123****8901'
akops doctor                        # 环境体检
```

## 文档

| 目录 | 读者 |
|---|---|
| [`docs/akops-design.md`](docs/akops-design.md) | 总设计（权威） |
| [`docs/ai/`](docs/ai/AGENTS.md) | AI 协作者 / 新开发者 |
| [`docs/dev/`](docs/dev/README.md) | 开发者 |
| [`docs/user/`](docs/user/README.md) | 最终用户 |

## License

MIT
