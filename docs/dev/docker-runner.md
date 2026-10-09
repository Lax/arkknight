# DockerRunner 部署指南（任务 6 后半，ADR-0001 D5）

mower 会话的两种运行形态：`process`（默认，本地 Python 检出）与 `docker`
（bollard 起停会话容器）。本文说明 DockerRunner 的镜像约定、Docker 端点
配置与容器化部署（arkknight 自身跑在容器里）的推荐接法。

## 1. 账号启用 DockerRunner

```toml
# accounts/<key>/account.toml
[schedule]
runner = "docker"        # process（默认） | docker
```

设备需提供容器网络视角的 adb 地址（`devices/<name>.toml`）：

```toml
[connection]
host_adb = "127.0.0.1:2781"      # ProcessRunner / 宿主侧 maa-cli 用
docker_adb = "arknights:5555"    # DockerRunner 的容器网络视角
docker_network = "arknights_default"  # 会话容器加入的网络
```

## 2. 镜像约定（与 arknights-mower 镜像一致）

DockerRunner 对镜像做如下假设（`MOWER_DATA_DIR=/mower`，工作目录 `/mower`）：

| 容器内路径 | 挂载来源（宿主/arkknight 工作目录） | 说明 |
|---|---|---|
| `/mower/config` | `accounts/<key>/mower/` | conf.yml/plan.json **直读直写**，UI 改动天然持久化 |
| `/mower/tmp` | `accounts/<key>/mower-data/tmp/` | 会话临时文件 |
| webview 端口 | 会话分配端口 1:1 发布到 `127.0.0.1` | conf.yml 由 arkknight 白名单改写（adb/port/token/start_automatically） |

启动命令固定 `python3 run_server.py`。镜像不存在时先构建：

```bash
cd ~/src/arknights-mower            # 或你的 mower 检出
docker build -t arknights-mower:latest .
```

更新流程走 `arkknight mower update`（pin commit）+ 重建镜像；
异常回滚 `arkknight mower rollback` 后再次重建。

## 3. Docker 端点（paths.docker_host）

bollard 直连 Docker API，**不依赖 docker CLI**：

| 场景 | 配置 |
|---|---|
| arkknight 跑在宿主机 | `docker_host = ""`（默认：Linux `/var/run/docker.sock`、Windows 命名管道） |
| arkknight 跑在容器里 | 挂载宿主 socket：`/var/run/docker.sock`（信任级别=root，见 §5）；或走 socket-proxy（§4） |
| 远程 Docker | `docker_host = "tcp://docker-host:2375"`（务必配 TLS 或置于内网） |

`arkknight doctor` 用同一通道探测（`daemon <版本>（endpoint <端点>），镜像 … 就绪`），
容器内没有 docker CLI 也能正确报告。

## 4. 容器化部署：socket-proxy（推荐）

arkknight 容器需要访问 Docker API 时，不要把整个 docker.sock 裸挂给业务容器
（等价 root）。用 socket-proxy 边车做动词级白名单——调度器只需要
`containers/*`、`images/*` 的只读+起停能力：

```yaml
# examples/socket-proxy.compose.yml —— 与 arkknight 服务同网络部署
services:
  socket-proxy:
    image: tecnativa/docker-socket-proxy
    environment:
      CONTAINERS: 1   # 容器列表/创建/启动/停止/删除
      IMAGES: 1       # 镜像检查（doctor 镜像就绪探测）
      POST: 1         # 起停类 POST 动词
    volumes:
      - /var/run/docker.sock:/var/run/docker.sock:ro
    networks: [internal]

  arkknight:
    image: ghcr.io/lax/arkknight:latest
    environment:
      ARKKNOTH_DOCKER_HOST: tcp://socket-proxy:2375
    networks: [internal]
```

`arkknight.toml`：

```toml
[paths]
docker_host = "tcp://socket-proxy:2375"
```

注意：socket-proxy 需要能解析容器网络的 `docker_adb`（如 `arknights:5555`），
因此 arkknight 与 redroid/会话容器应处于同一 Docker 网络或可路由网络。

## 5. 安全边界（设计 §13）

- 控制台**不透传** docker.sock：Docker API 仅在 arkknight 进程内经 bollard 使用。
- socket-proxy 白名单按最小集开放（CONTAINERS/IMAGES，不开 ALLOW_RESTARTS/SECRETS）。
- 会话容器命名 `arkknight-mower-s<session_id>`，drain 时 stop+remove，
  重跑幂等（同号残留先清理）。
