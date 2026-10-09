# 常见问题

先跑 `arkreunion doctor` —— 绝大多数环境问题它会直接指出缺什么、怎么修。

## 切号失败

### `MAA 任务匹配不到账号`

MAA 只能选**该设备已登录过**的账号。检查：

1. 该 (账号, 设备) 组合跑过 `arkreunion provision <key>` 了吗？
2. 登录时**勾选了「记住密码/快速登录」**吗？没勾就没有快速登录记录。
3. `account_name` 匹配串对不对？官服是**打码手机号片段**（如 `123****8901`），
   要与游戏登录界面显示的一致；B服用昵称。

```bash
arkreunion device test <device>     # 设备在线 + 游戏包在不在
arkreunion account show <key>       # 看 account_name 填的是什么
```

### 匹配串不够唯一，多账号时切错号

这是设计文档 §9.4 记录的真实故障：`account_name` 太泛会**静默登进错误账号**，
MAA 退出码仍是 0，arkreunion 无从察觉，然后在别人号上跑基建。

**填 `uid`** 即可核验。切号成功后会自动跑一次 UID 核验 pipeline
（主界面点头像 → OCR 个人信息页 → 期望 UID），不符即 `SwitchFailed` 并**阻止后续基建**。

```bash
arkreunion provision <key>         # 交互录入 uid
# 或控制台账号页「补 UID」
```

没配 uid 时 doctor 会提示串数据风险。

### `UID 核验失败`

报错会说「当前登录 UID 与配置 X 不符或核验超时，疑似登错号，已阻止后续操作」。
两种可能：

1. **真的登错号** —— `account_name` 匹配到了别的账号，检查配置
2. **核验 pipeline 的 roi 没校准** —— UID 核验点主界面左上角头像（roi `[0,0,130,100]`），
   不同分辨率/UI 缩放下可能点偏。首次使用建议校准 `render_uid_check_pipeline` 里的 roi

## adb 连不上

### `设备不可达：device 'xxx' not found`

网络 adb 需要先 `adb connect`。arkreunion 在 `device test`、`switch`、截图前都会自动
connect，但如果你直接用 adb：

```bash
adb connect <host_adb>
adb devices
```

如果 daemon 刚重启，adb server 会丢失连接记录，**首次操作必然失败一次**，重试即可
（arkreunion 内部已处理）。

### 容器内 `127.0.0.1` 连不上宿主

arkreunion 自己跑在容器里时，`host_adb` 填 `127.0.0.1:<port>` 会指向**容器自身**。
改填容器网络地址：

```bash
arkreunion device remove <name> --yes
arkreunion device add <name> --host-adb arknights:5555 --docker-adb arknights:5555
```

### `unauthorized`

设备端没授权这台 adb 主机。redroid 侧通常 `ro.adb.secure=0` 无此问题；
物理机/其他模拟器需在设备上确认授权弹窗，或删除 `~/.android/adbkey` 重连。

## 控制台打不开 / 白屏

### 页面能开但一片空白，数据全空

**token 不对。** 401 时侧栏会有红字提示，服务端返回的 `hint` 会指明配置文件路径。

```bash
grep token <workdir>/arkreunion.toml    # 确认服务端配的值
```

控制台左下角的 token 框要填**同一个值**。首次建议直接用
`http://127.0.0.1:7100/?token=<值>` 访问，前端会存入 localStorage。

### 改了 token 后 401「token 已提供但不一致」

localStorage 里还是旧值。清 localStorage 或回控制台改掉。

### 宿主访问不了（容器部署）

容器内 `bind = "127.0.0.1"` 收不到宿主端口转发。改成 `0.0.0.0` + 配 token
（设计 §17 强制要求），宿主侧仍只暴露 `127.0.0.1:7100`：

```toml
[server]
bind = "0.0.0.0"
port = 7100
token = "<openssl rand -hex 16>"
```

## 端口冲突

### `PortExhausted`

`mower_session_range`（默认 58100-58199）用尽了。`doctor` 会报告段内占用情况。

```bash
arkreunion status        # 看端口记账
ss -ltn | grep -E '581[0-9][0-9]'
```

改 `arkreunion.toml` 的 `[ports].mower_session_range` 换段，重启 daemon。

### 7100 被占

```bash
arkreunion server --port 7200
# 或改 arkreunion.toml 的 server.port
```

## 调度不启动

### 账号一直不被调度

检查「就绪集合」四条件全满足（设计 §10.3）：

1. `enabled = true`（`arkreunion account enable <key>`）
2. **当前处于某个时间窗内** —— 没配时间窗 = 不限时段；配了则只在窗内参与。
   注意时间窗用**游戏日界**（官服 UTC-4 的 04:00 刷新），不是本地零点
3. 无活跃会话（单设备串行，一次只有一个）
4. 不在退避中（失败后 5m 起指数退避，60m 封顶）

`arkreunion status` 看队列与退避状态。

### 时间窗没生效

- 必须是严格 `HH:MM`（`8:00` 不合法）
- `start < end`，**不支持跨午夜** —— `22:00-06:00` 会被拒绝，拆成两段
- 时区：调度计算按游戏日界，`scheduler.timezone` 只是展示用

### 调度器暂停了

`arkreunion schedule resume`（控制台总览页也有按钮）。

## 会话问题

### `当前有会话运行中`

M1 单设备串行，手动 `session start` 以 priority=100 插队但**不抢占**已 Running 会话 ——
会等它 Draining。等一会儿，或先 drain。

若确认无会话却报此错，可能是 daemon 重启前的残留：重启 daemon 会自动清理孤儿租约。

### 会话一直 running 不结束

两个超时源任一触发即 Draining：

- 时间片到期（`slice` / `default_slice`）
- 硬上限（`max_session_runtime`，默认 6h）

都正常却卡住，看日志：

```bash
arkreunion session logs <id> -f
```

### mower 会话起不来

```bash
arkreunion doctor        # python 3.11+？mower 检出在？
arkreunion session logs <id>
```

ProcessRunner 需要容器/宿主有 `python3`（3.11+）与 mower 检出，且**依赖已装**
（`pip install -r requirements.txt`）。

## 截图失败

`设备离线或屏幕未点亮` —— 先 `device test`，再确认设备屏幕没息屏（息屏时
`screencap` 返回空）。截屏是按需的，不会自动重试。

## 游戏更新后

1. 更新 APK 安装到模拟器
2. `arkreunion doctor` —— 确认游戏包仍能检出、资源版本是否匹配
3. `arkreunion maa update` —— MAA 资源可能需要跟进
4. 若 mower 报错，多半是 mower alpha 分支的 schema 变了：
   ```bash
   cd <mower_dir> && git pull
   arkreunion doctor        # doctor 能检出非 git 检出的问题
   ```
5. **切号仍失败**通常是 MAA 的资源与游戏版本不匹配（issue #15309 类问题：
   MAA 静默失败），更新 MAA 资源

## mower 相关

### `mower 不是 git 检出`

doctor 报这个说明 `mower_dir` 指向的目录没有 `.git`（常见于从镜像拷贝的检出）。
后果是 `arkreunion mower update` 不可用（要 git pin commit）。不影响运行。

要修复就换成真正的 clone：

```bash
git clone --depth=1 -b alpha https://github.com/ArkMowers/arknights-mower <mower_dir>
```

### mower 改了配置会丢吗

不会。会话启动时 `accounts/<key>/mower/` 通过 `MOWER_DATA_DIR` 的符号链接直挂进
mower 数据目录，**你在 mower UI 里的改动会持久化**。

arkreunion 只会白名单改写 `conf.yml` 的 4 个字段（adb / webview.port /
webview.token / start_automatically），其余不碰。

### `mower update` / `export` / `import` 报「尚未实现」

M1 未交付的命令，见设计文档 §20 里程碑。当前用 `cp -r <workdir>` 手动备份。

## 诊断技巧

```bash
arkreunion doctor              # 环境体检
arkreunion status              # 总览
arkreunion -v switch <key>     # debug 级日志
arkreunion -vv <cmd>           # trace
RUST_LOG=arkreunion=debug arkreunion server

tail -f <workdir>/logs/sessions/<id>.log    # 会话日志
cat <workdir>/state/daemon.json              # daemon 运行信息（pid/port/启动时间）
```

会话事件全量记在 SQLite `session_events` 表：

```bash
sqlite3 <workdir>/state/arkreunion.db "SELECT ts_ms, kind, detail FROM session_events ORDER BY id DESC LIMIT 20"
```

## 还是不行

1. `arkreunion -vv <失败的命令>` 看完整日志
2. 查设计文档对应章节（`docs/arkreunion-design.md`）
3. 提 issue 时附上：`doctor` 输出、`status` 输出、失败命令与完整日志、arkreunion 版本