# 部署指南

从「装好二进制」到「长期无人值守运行」：部署拓扑选择、systemd / 容器化、
redroid 设备镜像升级、备份与升级。单机跑通见 [`quickstart.md`](./quickstart.md)。

## 部署拓扑

| 拓扑 | 适用 | 组成 |
|---|---|---|
| **主机直装** | Linux 主机 + 本机模拟器/Docker | 二进制 + systemd，mower 以 ProcessRunner 跑 |
| **容器化**（推荐） | 服务器长期无人值守 | arkknight 容器 + redroid 容器 + 可选 socket-proxy |
| **混合** | 已有 redroid 容器、arkknight 装宿主 | 主机直装 + `host_adb` 指向容器映射端口 |

无论哪种拓扑，**所有状态都在工作目录**（`arkknight.toml` + `devices/` +
`accounts/` + `state/` + `logs/`）——备份 = 打包这个目录。

## 主机直装（systemd）

```bash
# 1) 安装二进制（见 install.md），初始化工作目录
arkknight init --dir /srv/arkknight/workdir
# 2) 配好设备/账号/时间窗（见 quickstart.md），跑绿 doctor
arkknight doctor
```

`/etc/systemd/system/arkknight.service`：

```ini
[Unit]
Description=arkknight 多账号调度 daemon
After=network-online.target docker.service

[Service]
Type=simple
User=arkknight
WorkingDirectory=/srv/arkknight/workdir
ExecStart=/usr/local/bin/arkknight server
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
```

```bash
systemctl enable --now arkknight
journalctl -u arkknight -f        # 日志（tracing 输出）
```

> 暴露到局域网必须在 `arkknight.toml` 配 `server.token`（非 loopback 绑定
> 空 token 拒绝启动，§17）。宿主自用保持默认 `127.0.0.1:7100`。

## 容器化部署

arkknight 自己跑在容器里时，两个关键点：

1. **镜像要带运行依赖**：ProcessRunner 会话在容器内 spawn mower（Python 检出），
   所以镜像 = mower 运行环境 + arkknight 二进制 + `maa` + `adb`。
2. **网络视角**：容器内的 `127.0.0.1` 是容器自己——设备 `host_adb` 要填
   **容器网络地址**（如 `arknights:5555`），与 redroid 同一 Docker 网络。

示例 Dockerfile（在 mower 镜像之上叠加；按你的 mower 镜像调整路径）：

```dockerfile
# 基础镜像需含：python3 + mower 依赖 + /mower 检出
FROM arknights-mower:latest
# arkknight 单二进制（ui 已嵌入，宿主上 cargo install --path crates/arkknight 得到）
COPY arkknight /usr/local/bin/arkknight
# maa-cli（切号 INV-1 唯一依赖）
COPY --from=maa-cli /usr/local/bin/maa /usr/local/bin/maa
RUN apt-get update && apt-get install -y --no-install-recommends adb && rm -rf /var/lib/apt/lists/*
ENTRYPOINT ["/usr/local/bin/arkknight"]
```

compose 要点（完整可运行示例见 [`examples/socket-proxy.compose.yml`](../../examples/socket-proxy.compose.yml)）：

```yaml
services:
  arkknight:
    image: arkknight:latest
    command: server --workdir /data
    ports:
      - "127.0.0.1:7100:7100"        # 控制台只暴露给宿主 loopback
    volumes:
      - ./workdir:/data
    networks: [arknights]             # 与 redroid 同网络，docker_adb 才可路由

  # 容器内绑定与鉴权写进挂载的 workdir/arkknight.toml：
  #   [server] bind = "0.0.0.0"（容器内必须绑非 loopback）
  #            token = "<强随机token>"（§17：非 loopback 强制 token）
  # 宿主侧端口映射仍只绑 127.0.0.1。
```

> 工作目录以卷挂载（`./workdir:/data`）——容器可丢，状态不可丢。

**arkknight 容器需要操作 Docker（DockerRunner / 起停容器）时**，不要裸挂
`docker.sock`：用 socket-proxy 边车按动词白名单开放，`paths.docker_host` 指向它。
完整做法见 [`../dev/docker-runner.md`](../dev/docker-runner.md)。

## redroid 设备镜像升级（APK / 资源）

redroid 的游戏数据（APK + 资源）在容器文件系统里，**升级 = 改完数据后
`docker commit` 固化成新 tag**，之后重建容器都基于新 tag：

```bash
# 1) 暂停调度，确认无活跃会话（会话边界会强停游戏，见下方警告）
arkknight schedule pause

# 2) 更新 APK（官方直链 https://ak.hypergryph.com/downloads/android_lastest）
adb -s <设备> install -r arknights-hg-<ver>.apk

# 3) 启动游戏完成资源热更下载（约 10-20GB，几十分钟）
#    ⚠️ 全程保持调度暂停——中途被强停会留下「数据文件过期」（见 FAQ）
adb -s <设备> shell monkey -p com.hypergryph.arknights -c android.intent.category.LAUNCHER 1

# 4) 下载完成（进到登录界面）后停游戏、固化镜像
adb -s <设备> shell am force-stop com.hypergryph.arknights
docker commit <redroid容器> <你的仓库>:<设备>-update-$(date +%y%m%d)

# 5) 之后的容器重建/迁移都基于新 tag
arkknight schedule resume
```

升级后跑一次 `arkknight doctor` 确认游戏包可检出；游戏进主界面后首次访问
新内容时可能还有少量按需资源下载，属正常。

## 升级 arkknight 本体

```bash
git pull && (cd ui && npm ci && npm run build) && cargo install --path crates/arkknight
systemctl restart arkknight   # 或重建容器
arkknight doctor              # 升级后必跑
```

- SQLite schema 在打开时自动迁移（`user_version` 版本化），老工作目录直接可用
- 配置字段向后兼容：新字段有默认值，旧 `arkknight.toml` 不需要手改
- 版本差异看 [Releases](https://github.com/Lax/arkknight/releases) 的变更说明

## 备份与迁移

```bash
# 备份（停 daemon 可选——SQLite 为 WAL 模式，运行中打包也安全，但停更稳）
tar czf arkknight-backup-$(date +%F).tar.gz \
    -C /srv/arkknight/workdir \
    arkknight.toml devices/ accounts/ state/ templates/

# 恢复 = 解包到新机工作目录 + 装好依赖（install.md）+ doctor
```

- `accounts/<key>/mower/` 是 mower 配置的**事实源**（会话直读直写）——备份它
  等于备份用户在 mower UI 里的全部改动
- `logs/` 可不备（可重建）；`state/arkknight.db` 含会话历史与登录亲和记录，建议备
- 跨机迁移：解包后检查 `paths.*` 与设备地址是否需要改，`arkknight doctor` 跑绿即可

## 安全清单

- [ ] 控制台端口只绑 `127.0.0.1`（或强 token + 防火墙）
- [ ] 非 loopback 部署必配 `server.token`（强随机，非示例值）
- [ ] **不把 docker.sock 裸挂给业务容器**——用 socket-proxy 白名单
      （[`../dev/docker-runner.md`](../dev/docker-runner.md) §4）
- [ ] 工作目录权限收敛（含各账号的 mower bundle 与 skland 凭据）
- [ ] 自动化工具风险自担；账号共享/自动化可能违反游戏用户协议
