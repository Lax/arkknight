# arkreunion（塔露拉）—— 方舟多账号调度中心

> 整合运动的领袖负责「整合」；arkreunion 负责整合你的方舟自动化：**MAA × mower × 多账号**，一个二进制全管。

跨平台（Linux / Windows / macOS）的明日方舟**多账号编排器**：在少量安卓设备（redroid、MuMu、雷电、物理机等）上，编排 [MAA](https://github.com/MaaAssistantArknights/MaaAssistantArknights)（账号切换 / 任务）与 [arknights-mower](https://github.com/ArkMowers/arknights-mower)（基建排班），按时间窗自动轮转运行多个游戏账号。

**arkreunion 自身不做任何游戏内图像识别与操作自动化**——账号切换 100% 调用 MAA 官方能力（INV-1）。

## 特性

- **多账号轮转**：按时间片与优先级自动排队、占用设备、切号、跑基建、释放
- **切号防串数据**：MAA 匹配串可能匹配到错误账号（静默、退出码仍为 0），配 `uid` 后切号成功即 OCR 核验登录身份，不符即阻止后续操作
- **单二进制 + Web 控制台**：控制台 UI 编译期嵌入，`arkreunion server` 即用
- **配置即事实源**：账号/设备/策略都在工作目录文件树里，可读可版本控制
- **失败不空转**：指数退避（5m → 60m 封顶）+ 双超时 + 看门狗

## 依赖

adb（platform-tools）、[maa-cli](https://github.com/MaaAssistantArknights/maa-cli) + MaaCore、Python 3.11+、[arknights-mower](https://github.com/ArkMowers/arknights-mower) alpha 检出。`arkreunion doctor` 逐项体检。

## 安装

```bash
# 从 Release 下载解压（三平台安装包 + deb + Arch 包），或源码安装：
cd arkreunion && cd ui && npm ci && npm run build && cd ..   # 必须：UI 在编译期嵌入
cargo install --path crates/arkreunion
```

详见 [`docs/user/install.md`](docs/user/install.md)。

## 快速开始

```bash
arkreunion init --dir ~/arkreunion && cd ~/arkreunion
arkreunion doctor                                    # 环境体检

arkreunion device add redroid-main --host-adb 127.0.0.1:2771
arkreunion device test redroid-main
arkreunion account add main --server official \
    --account-name '123****8901' --uid 1000123456

arkreunion provision main      # 人工登录一次（切号前提，MAA 只能选已登录过的账号）
arkreunion switch main         # 验证切号
arkreunion server --open       # 常驻调度 + Web 控制台 http://127.0.0.1:7100
```

四个标识字段别混：`key`（本地定位键）· `display_name`（展示）· `account_name`（切号匹配串）· `uid`（游戏身份核验）。

完整版见 [`docs/user/quickstart.md`](docs/user/quickstart.md)。

## 文档

| 目录 | 读者 | 入口 |
|---|---|---|
| [`docs/`](docs/README.md) | 文档中心（按受众索引） | [arkreunion-design.md](docs/arkreunion-design.md) 是**唯一权威设计** |
| [`docs/user/`](docs/user/README.md) | 最终用户 | [install](docs/user/install.md) · [quickstart](docs/user/quickstart.md) · [config](docs/user/config.md) · [console](docs/user/console.md) · [faq](docs/user/faq.md) |
| [`docs/dev/`](docs/dev/README.md) | 开发者 | [api](docs/dev/api.md) · [testing](docs/dev/testing.md) · [release](docs/dev/release.md) |
| [`docs/ai/`](docs/ai/AGENTS.md) | AI 协作者 / 新开发者 | [AGENTS.md](docs/ai/AGENTS.md) · [ADR](docs/ai/adr/) |

## 当前状态

🚧 开发中（M1 里程碑）。**已实现**：调度器（时间片轮转/优先级/退避/看门狗）、账号切换（INV-1 唯一路径）、UID 核验防串数据、provision、mower ProcessRunner 会话、Web 控制台七页、设备截图、三平台发布流水线。

**未做**：mower DockerRunner、RedroidDocker 设备后端（多设备池 M2）、`export`/`import`、OpenAPI、统计页、调度策略表单编辑、72h 真机验收。

完整的已实现/未做清单见 [AGENTS.md](docs/ai/AGENTS.md) 的状态段；命令可用性见 [`docs/dev/api.md`](docs/dev/api.md)。

> ⚠️ 使用自动化工具的风险由用户自担；请勿用于代练等商业用途。账号共享/自动化可能违反游戏用户协议。

## License

MIT