# 安装

arkknight 是单二进制 + Web 控制台：装好二进制后所有状态落在**工作目录**里，卸载即删目录。

## 依赖

| 依赖 | 必需 | 说明 |
|---|---|---|
| adb（platform-tools） | ✅ | 设备连通与切号 |
| [maa-cli](https://github.com/MaaAssistantArknights/maa-cli) + MaaCore | ✅ | **账号切换的唯一实现**（INV-1，不可替换） |
| Python 3.11+ | ✅ | mower ProcessRunner 会话（纯 DockerRunner 可免） |
| [arknights-mower](https://github.com/ArkMowers/arknights-mower)（alpha 分支检出） | ✅ | 基建排班 |
| Docker | ⭕ | 可选：mower 会话容器化（DockerRunner）、redroid 建池（M2）。见 [`deploy.md`](./deploy.md) |

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

## 方式三：Docker / 容器化

Release 未分发镜像，容器化部署自行构建：**mower 运行镜像 + arkknight 二进制 +
maa-cli + adb**。关键点（容器内 bind 必须 `0.0.0.0` + 配 token 等）与
compose/redroid 组网、socket-proxy、镜像升级流程，见专文
[`deploy.md`](./deploy.md)；mower 会话容器化（DockerRunner）另见
[`../dev/docker-runner.md`](../dev/docker-runner.md)。

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

装好后按 [`quickstart.md`](./quickstart.md) 从零跑通；长期运行/服务器部署见
[`deploy.md`](./deploy.md)。