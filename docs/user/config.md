# 配置手册

三类配置，权威定义在 [`../arkreunion-design.md` §11](../arkreunion-design.md)，本文面向使用者补充说明。

| 配置 | 位置 | 改完生效方式 |
|---|---|---|
| 全局策略 | `<workdir>/arkreunion.toml` | 改完 `arkreunion server` 重启 |
| 账号 | `<workdir>/accounts/<key>/account.toml` | 部分可热改（`account enable/disable`），其余需停daemon |
| 设备 | `<workdir>/devices/<name>.toml` | 需停 daemon |

改配置前建议先 `arkreunion export` 备份（M2 交付，当前可用 `cp -r` 整目录）。

## 全局：arkreunion.toml

### `[paths]`

| 字段 | 默认 | 说明 |
|---|---|---|
| `adb_path` | `adb` | `init` 探测到绝对路径时会写绝对路径（避免 PATH 差异） |
| `maa_dir` | `~/.local/share/arkreunion/maa` | maa-cli 安装根；`arkreunion maa install/update` 用 |
| `mower_dir` | `~/src/arknights-mower` | mower 检出（ProcessRunner 需要）。**改完跑 doctor** —— mower alpha 会动 schema |
| `docker_mower_image` | `arkreunion-mower:latest` | DockerRunner 用（M2） |

### `[server]`

| 字段 | 默认 | 说明 |
|---|---|---|
| `bind` | `127.0.0.1` | 监听地址。**改非 loopback 必须配 token，否则拒绝启动**（设计 §17） |
| `port` | `7100` | 控制台与 API 端口 |
| `token` | `""`（不鉴权） | 非空时启用。`Authorization: Bearer <token>` 或 `?token=<token>` |

**容器化部署**：`bind` 必须设 `0.0.0.0`（容器内绑 loopback 收不到宿主端口转发），
同时按 §17 强制配 token。宿主侧仍只把端口绑到 `127.0.0.1`：

```yaml
ports:
  - "127.0.0.1:7100:7100"    # 只暴露到宿主 loopback
```

生成 token：`openssl rand -hex 16`。

### `[scheduler]`

| 字段 | 默认 | 说明 |
|---|---|---|
| `timezone` | `Asia/Shanghai` | **仅展示用**；调度计算按游戏日界 |
| `game_day_boundary` | `04:00` | 游戏日界（官服 UTC-4 的 04:00 刷新）。不要按本地零点写 |
| `default_slice` | `2h` | 默认时间片；账号可用 `--slice` 覆盖 |
| `max_session_runtime` | `6h` | 单会话硬上限（第二超时源） |
| `max_switch_retries` | `2` | 切号最大重试次数（退避重试） |
| `daily_guarantee` | `true` | 每账号每日至少一个完整时间片 |
| `drain_grace` | `2m` | 会话优雅停止等待，超时后 pid 兜底强杀 |
| `watchdog_interval` | `30s` | 执行器健康探测间隔 |
| `watchdog_threshold` | `3` | 连续失败达此数 → Draining |
| `backoff.initial` / `.max` / `.factor` | `5m` / `1h` / `2.0` | 失败指数退避参数 |
| `cross_account` | `false` | M3 跨账号交叉调度开关 |

**两个超时源**：时间片到期（`slice_deadline`）与硬上限（`max_runtime_deadline`），
任一触发即 Draining。

### `[ports]`

| 字段 | 默认 | 说明 |
|---|---|---|
| `mower_session_range` | `[58100, 58199]` | mower 会话 Web UI 端口段，按序分配并记账 |
| `redroid_host_range` | `[28000, 28099]` | redroid 宿主端口段（M2） |

端口分配会**探测系统占用**并记入 SQLite，跨会话不重复。`doctor` 会检查段内是否空闲。

### `[device_defaults.redroid]`（M2）

镜像标签、内存/CPU 上限、GPU 开关、扩容准入水位。当前 M1 不生效，保留占位。

## 账号：accounts/<key>/account.toml

```toml
key = "main"                          # 本地定位键（= 目录名，勿手改）
display_name = "主号"                  # 仅展示
server = "official"                   # official | bilibili
account_name = "123****8901"          # MAA 切号匹配串
uid = "1000123456"                    # 游戏 UID（可缺省）
enabled = true

[schedule]
priority = 50                         # 0-100，同一时刻竞争时的抢占顺序
slice = "90m"                         # 覆盖全局 default_slice

[[schedule.windows]]
start = "08:00"                       # 严格 HH:MM，24 小时制
end = "12:00"                         # 须 start < end，不跨午夜
executor = "mower"                    # mower | maa
task = "roguelike"                    # 仅 maa 窗口需要
```

**`key` 是本地定位键，不是游戏身份。** 游戏身份是 `account_name`（切号匹配）与 `uid`（核验）。

CLI 改这些字段（避免手改出错）：

```bash
arkreunion account enable main        # / disable
arkreunion account show main          # 原样打印文件
```

`uid` 缺省时切号跳过身份核验，doctor 提示串数据风险。可用 `provision` 录入，
或控制台账号页「补 UID」按钮内联补齐。

### 时间窗规则

- 严格 `HH:MM`（`8:00` 不合法，要 `08:00`）
- `start < end`，**不支持跨午夜**。跨午夜请拆成两段
- `executor = maa` 时必须给 `task`（`accounts/<key>/maa/tasks/` 下的任务名），否则拒绝调度
- 不填时间窗 = 不限时段，按优先级参与轮转

## 设备：devices/<name>.toml

```toml
name = "redroid-main"
backend = "external"                  # M1 仅 external（已存在、只连不管）

[connection]
host_adb = "127.0.0.1:2771"           # 宿主视角
docker_adb = "arknights:5555"          # 容器网络视角（可选）
docker_network = "arknights_default"  # DockerRunner 需加入的网络（可选）

notes = "1920x1080@280，nvidia GPU"
```

**双地址规则**：同一台设备对宿主进程与容器内进程呈现不同 adb 地址。

- `host_adb` → ProcessRunner 与宿主侧 maa-cli 使用
- `docker_adb` → DockerRunner 使用（未实现，需两者都填才算「Docker 兼容」）

arkreunion 自身跑在容器里时，`host_adb` 要填**容器网络地址**（`arknights:5555`），
因为容器内 `127.0.0.1` 指向容器自身。

## 会话自动生成的配置

arkreunion 会在会话启动瞬间物化/改写以下文件（§11.4）：

| 文件 | 改写内容 |
|---|---|
| `accounts/<key>/maa/profiles/default.toml` | 切号前每次改写 `connection.address` 为当前 adb |
| `accounts/<key>/maa/tasks/startup.toml` | StartUp 任务 + `account_name`（INV-1） |
| `accounts/<key>/mower/conf.yml` | 白名单改写 `adb` / `webview.port` / `webview.token` / `start_automatically` |

`tasks/` 内你自己加的任务文件**永不触碰**；mower 的 `conf.yml`/`plan.json` 通过
`mower-data` 符号链接直挂，所以**你在 mower UI 里的改动会持久化**。

> 物化产物可重建（删了下次会话自动生成）。唯一例外：mower bundle 里你通过 UI 改的内容。

## 环境变量与命令行覆盖

| 方式 | 说明 |
|---|---|
| `--workdir <DIR>` | 显式指定工作目录（默认从 cwd 向上探测 `arkreunion.toml`） |
| `--config <FILE>` | 用替代配置文件，其父目录即工作目录 |
| `--maa-dir` / `--mower-dir` | 单次覆盖 `[paths]` |
| `ARKOPS_MOWER_DIR` | mower 探测的环境变量（`init`/`doctor` 识别） |
| `MOWER_DATA_DIR` | ProcessRunner 会话设置，指向 `accounts/<key>/mower-data` |
| `MAA_CONFIG_DIR` | maa-cli 配置目录（arkreunion 按账号设置） |
| `RUST_LOG` / `-v` / `-vv` | 日志级别（`info`/`debug`/`trace`） |

## 配置改了怎么验证

```bash
arkreunion doctor        # 路径、账号一致性、端口段
arkreunion status        # 账号/设备/会话/队列/退避
```