# 发布流程（三平台安装包）

自动化入口：`.github/workflows/release.yml`。**推送 tag `v*` 即自动产出全部安装包并发
GitHub Release**；也支持 Actions 页手动触发（workflow_dispatch，产物仅存 workflow
artifacts，用于演练）。

## 产物矩阵

| 目标 | 包 | 构建方式 |
|---|---|---|
| x86_64-unknown-linux-gnu | `arkknight-v{ver}-x86_64-unknown-linux-gnu.tar.gz` + `.deb` | ubuntu 原生 |
| aarch64-unknown-linux-gnu | `arkknight-v{ver}-aarch64-unknown-linux-gnu.tar.gz` | ubuntu **交叉编译**（`gcc-aarch64-linux-gnu` + `CC_aarch64_unknown_linux_gnu`，rusqlite bundled 的 SQLite C 随之交叉） |
| x86_64-pc-windows-msvc | `arkknight-v{ver}-x86_64-pc-windows-msvc.zip` | windows 原生 |
| aarch64-apple-darwin | `arkknight-v{ver}-aarch64-apple-darwin.tar.gz` | macos-latest（Apple Silicon）原生 |
| x86_64-apple-darwin | `arkknight-v{ver}-x86_64-apple-darwin.tar.gz` | macos-latest **交叉**（Apple 工具链原生支持，rustup target 即可） |
| Arch Linux x86_64 | `arkknight-{ver}-1-x86_64.pkg.tar.zst` | archlinux 容器内 makepkg **重打包**预编译二进制（`options=('!strip' '!debug')`） |
| Gentoo (binary overlay) | `games-util/arkknight-bin` ebuild + overlay 骨架 | CI 按版本生成（`-bin` 式，SRC_URI 指向 Release tar.gz） |
| 全部 | `SHA256SUMS` | release job 汇总生成 |

## Gentoo overlay（外部项目用法）

gentoo job 产出 `overlay/` 目录（artifact + 随 Release 附带），结构：
`profiles/repo_name` + `metadata/layout.conf`（thin-manifests）+
`games-util/arkknight-bin/arkknight-bin-{ver}.ebuild`。整目录 push 成独立 overlay 仓库即可：

```bash
# 用户侧（/etc/portage/repos.conf/arkknight.conf）
[arkknight]
location = /var/db/repos/arkknight
sync-type = git
sync-uri = https://github.com/<org>/arkknight-overlay.git
```

注意：thin-manifests 模式无需 Manifest DIST 条目；ebuild 的 SRC_URI 指向
Release 的 linux x86_64 tar.gz，故发布先于 overlay 可安装。

每个包内含：单二进制 `arkknight`（**控制台 UI 已在编译期嵌入**，`arkknight server` 即用）+ README + LICENSE。

## 流程要点

1. **ui job 先行**：`npm ci && npm run build` 产出 `ui/dist`，经 artifact 传给各构建 job——
   release 构建的 rust-embed 在**编译期**嵌入 UI，缺 `ui/dist` 会编译失败（这是特性：
   保证发布物一定带最新控制台）。
2. **版本号**：来自 tag（`v0.1.0` → 包名 `arkknight-v0.1.0-*`）；二进制 `arkknight --version` 取自
   `CARGO_PKG_VERSION`（发版前记得同步 `workspace.package.version`）。
3. **deb**：仅 linux x86_64；配置见 `crates/arkknight/Cargo.toml` 的
   `[package.metadata.deb]`（cargo-deb）。
4. **校验和**：`SHA256SUMS` 覆盖全部包，随 Release 附上。

## 操作步骤（首次发布）

```bash
# 1. 推送仓库到 GitHub（工作流依赖 Actions）
git remote add origin git@github.com:<org>/arkknight.git && git push -u origin master
# 2. 发版（先确认 workspace version 与 tag 一致）
git tag v0.1.0 && git push origin v0.1.0
# 3. Actions → release → 等待 → Releases 页取安装包
```

## 本地演练（不推 tag）

- 构建发布二进制：`cd ui && npm run build && cargo build --release -p arkknight`
- Docker 里演练 aarch64 交叉编译（Gentoo 主机无交叉 gcc 时）：
  `docker run --rm -v $PWD:/work -w /work rust:1-bookworm bash -c
   "apt-get update && apt-get install -y gcc-aarch64-linux-gnu && rustup target add
   aarch64-unknown-linux-gnu && CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc
   CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc cargo build --release
   --target aarch64-unknown-linux-gnu -p arkknight"`

## 未决（后续批次）

- Docker 镜像分发（设计 §16）：`Dockerfile` 待随 registry 配置落地后接入 release 流水线
- macOS x86_64（Intel Mac）如需支持，在矩阵中追加 `x86_64-apple-darwin`（macos-13）
