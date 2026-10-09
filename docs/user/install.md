# 安装

arkknight 是单二进制 + Web 控制台：装好二进制后所有状态落在**工作目录**里，卸载即删目录。

## 依赖

| 依赖 | 必需 | 说明 |
|---|---|---|
| adb（platform-tools） | ✅ | 设备连通与切号 |
| [maa-cli](https://github.com/MaaAssistantArknights/maa-cli) + MaaCore | ✅ | **账号切换的唯一实现**（INV-1，不可替换） |
| Python 3.11+ | ✅ | mowe ProcessRunner 会话 |
| [arknights-mower](https://github.com/ArkMowers/arknights-mower)（alpha 分支检出） | ✅ | 基建排班 |
| Docker | ❌ | 仅 M2 的 RedroidDocker 建池需要；M1 用 ProcessRunner |

`arkknight doctor` 会逐项体检并给出缺什么、怎么补。

## 方式一：Release 安装包

GitHub Releases 提供：

| 平台 | 包 |
|---|---|
| Linux x86_64 | `arkknight-v*-x86_64-unknown-linux-gnu.tar.gz` / `.deb` |
| Linux aarch64 | `arkknight-v*-aarch64-unknown-linux-gnu.tar.gz` |
| Windows x86_64 | `arkknight-v*-x86_64-pc-windows-msvc.zip` |
| macOS | `arkknight-v*-aarch64-apple-darwin.tar.gz` / `x86_64-apple-darwin.tar.gz` |
| Arch Linux | `arkknight-*-x86_64.pkg.tar.zst` |

每个包是**单二进制**，控制台 UI 已在编译期嵌入（`arkknight server` 即用）。

验证 `SHA256SUMS`。

## 方式二：源码安装

```bash
git clone https://github.com/<org>/arkknight && cd arkknight
cd ui && npm ci && npm run build && cd ..   # 必须先构建 UI
cargo install --path crates/arkknight
```

> ⚠️ 跳过 `npm run build` 会导致编译失败 —— 这是**特性**：rust-embed 在编译期嵌入
> `ui/dist`，缺 dist 时构建即报错，保证发布物一定带最新控制台。

Arch Linux 用户可用 ebuild（`games-util/arkknight-bin`），见 [`../dev/release.md`](../dev/release.md)。

## 方式三：Docker

Docker 镜像在 M1 未随Release 分发（M2 交付）。容器化部署需自行构建，两点注意：

1. **运行层要带 mower 与 maa-cli**：M1 的 ProcessRunner 会在容器内 `spawn python3 run_server.py`，
   所以镜像必须有 Python + mower 检出 + `maa`（`/MAA`）+ `adb`。可基于
   `arknights-mower` 镜像叠加 arkknight 二进制。
2. **`server.bind` 必须是 `0.0.0.0` 而非 `127.0.0.1`**：容器内绑 loopback 收不到宿主端口
   转发。此时设计 §17 强制要求配 `server.token`，宿主侧仍只把端口绑到 `127.0.0.1`。

参考实现见 `/srv/reunion`（本机部署栈，含 compose 与凭据管理）。

## 依赖安装要点

### adb

```bash
# Arch
sudo pacman -S android-tools
# Debian/Ubuntu
sudo apt install adb
```

`arkknight init` 会把探测到的绝对路径写入 `[paths].adb_path`（避免 PATH 差异）。

### MAA

```bash
# maa-cli
cargo install maa-cli           # 或从 Releases 下载
maa install                     # 装 MaaCore + 资源
arkknight maa version          # 确认
```

也可直接用 `arkknight maa install` / `arkknight maa update`。

### mower

```bash
git clone --depth=1 -b alpha https://github.com/ArkMowers/arknights-mower ~/src/arknights-mower
cd ~/src/arknights-mower && pip install -r requirements.txt
```

`init` 会在 `~/src/arknights-mower`、`~/arknights-mower`、`./arknights-mower`、
`$ARKOPS_MOWER_DIR` 中依次探测；也可 `--mower-dir` 或配置 `paths.mower_dir` 指定。

> mower alpha 分支会动配置 schema（如 device 段迁移）。**更新 mower 后先跑 `doctor`**。

## 下一步

装好后按 [`quickstart.md`](./quickstart.md) 从零跑通。