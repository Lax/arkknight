# 快速上手

从零到跑通：装好依赖后约 10 分钟。命令按顺序执行，每步都有预期输出。

## 1. 初始化工作目录

```bash
arkknight init --dir ~/arkknight
```

产出目录骨架 + `arkknight.toml`（自动探测 adb/maa/mower/python/docker 写入 `[paths]`）。
**拒绝覆盖已初始化的目录** —— 换配置请改文件，不要重跑 init。

输出示例：

```
初始化工作目录 /root/arkknight …
  ✓ adb        /usr/bin/adb（Android Debug Bridge version 1.0.41）
  ✓ maa-cli    /usr/bin/maa（v0.7.5，MaaCore v6.17.5）
  ✓ mower      /root/src/arknights-mower（alpha@a234804）
  ✓ python     Python 3.12.3
  ✓ docker     daemon 27.1.1
  ✓ 配置       /root/arkknight/arkknight.toml
```

mower 探测不到就 `--mower-dir` 指定，或事后改 `paths.mower_dir`。

## 2. 环境体检

```bash
cd ~/arkknight            # 之后 CLI 从 cwd 向上探测 arkknight.toml
arkknight doctor
```

逐项检查 adb / 设备 / maa-cli / mower / python / docker / 账号唯一性 / 端口段。
**每项失败都带 `↳` 提示怎么修。** 先把 doctor 跑绿再往下走。

## 3. 注册设备

```bash
arkknight device add redroid-main --host-adb 127.0.0.1:2771
arkknight device test redroid-main
```

`device test` 会 `adb connect` + 查状态 + 检测游戏包，应输出：

```
设备 redroid-main（127.0.0.1:2771）：在线（device）
  游戏包: com.hypergryph.arknights
```

容器化部署补充：若 arkknight 本身跑在 Docker 里，`host_adb` 要填**容器网络地址**
（如 `arknights:5555`），因为容器内的 `127.0.0.1` 指向容器自身。

## 4. 注册账号

```bash
arkknight account add main \
  --server official \
  --account-name '123****8901' \
  --uid 1000123456
```

四个字段分工（**别混**）：

| 参数 | 是什么 | 作用 |
|---|---|---|
| `main` | **账号 key**：本地定位键，决定目录 `accounts/main/` | CLI 参数、内部标识 |
| `--server` | `official` / `bilibili` | 影响 MAA client_type、游戏包名、资源 |
| `--account-name` | **切号匹配串**：官服=打码手机号片段，B服=昵称 | MAA 在快速登录列表里按它选号 |
| `--uid` | 游戏内数字 UID | 切号后 OCR 核验身份，防登错号 |

- `account_name` 须在该设备已登录账号中唯一，重复会被拒绝。
- `--uid` 可省略（之后 `provision` 或控制台补），但省略则**切号后不做身份核验**，
  MAA 若匹配到错误账号不会被发现 —— 多账号场景强烈建议填。
- 多账号就注册多个 key，各自的 `--account-name` 要能区分开。

## 5. 人工登录一次（切号前提）

```bash
arkknight provision main
```

MAA 只能选择**该设备上已登录过**的账号，所以每个 (账号, 设备) 组合要先人工登录一次。
命令会输出投屏指引，按提示在投屏里登录并勾选「记住密码/快速登录」，然后回终端：

1. 投屏：`scrcpy -s 127.0.0.1:2771`（或 ws-scrcpy）
2. 拉起游戏：`adb -s 127.0.0.1:2771 shell monkey -p com.hypergryph.arknights -c android.intent.category.LAUNCHER 1`
3. 登录目标账号，**勾选记住密码/快速登录**
4. 回终端按回车确认，并录入 UID（可回车跳过）

成功后记录 `logins(main, redroid-main, provisioned)`，此后可自动切号。

> `provision` 会持有设备租约，与调度互斥。同一账号在多台设备上要分别 provision。

## 6. 验证切号

```bash
arkknight switch main
```

这是 INV-1 的唯一路径：`force-stop` 游戏 → 物化 MAA 配置 → `maa run startup -p default --batch`
→ 重试退避 → 记 `switch_log`。成功后再切一次就知道匹配串是否唯一了。

配了 `--uid` 时，startup 成功后还会跑一次 UID 核验 pipeline（主界面点头像 → OCR 个人信息页
→ 期望 UID）。不符即 `SwitchFailed`，**阻止后续基建操作** —— 这正是防串数据的关键。

## 7. 手动开会话

```bash
arkknight session start main --slice 90m     # 起 mower 基建会话
arkknight session list                        # 看会话
arkknight session logs <id> -f                # 跟踪日志
arkknight session stop <id>                   # 优雅停止（超时强杀兜底）
```

会话 = 「占用设备 → 切号 → 运行 mower/MAA → 释放」的完整过程。会话期间设备被独占，
其他账号排队。手动 `session start` 以 priority=100 插队，但**不抢占**已 Running 的会话。

mower 会话启动时 arkknight 会白名单改写 `conf.yml` 的 adb/端口/token/start_automatically，
并把它所在的 `accounts/<key>/mower/` 通过 `MOWER_DATA_DIR` 符号链接挂进 mower 数据目录 ——
所以**你在 mower UI 里的改动会持久化**，下次会话还在。

## 8. 常驻调度 + 控制台

```bash
arkknight server --open
```

后台常驻：调度引擎（15s 轮询）+ REST API + Web 控制台（<http://127.0.0.1:7100>）。

- 多账号按时间片轮转，优先级高的先
- 未设时间窗的账号不限时段参与轮转
- 失败任务指数退避（5m → 60m 封顶），不空转重试

配了 `server.token` 时访问 <http://127.0.0.1:7100/?token=<值>>，前端会存 localStorage。

其他常用命令：

```bash
arkknight status                # 总览：账号/设备/活跃会话/队列/退避
arkknight schedule pause        # 暂停自动调度（运行中会话不受影响）
arkknight schedule resume
```

## 多账号示例

```bash
arkknight account add main  --server official --account-name '123****8901' --uid 1000123456
arkknight account add alt   --server official --account-name '138****0002' --uid 1000999888
arkknight account add bili  --server bilibili --account-name '小号昵称'

# 时间窗：只在指定时段参与轮转（不跨午夜）
arkknight account add night --server official --account-name '156****0033' \
  --window '00:00-06:00:mower'

arkknight provision main && arkknight provision alt && arkknight provision bili
arkknight server
```

---

## 下一步

- 配置细节：[`config.md`](./config.md)
- 控制台七页说明：[`console.md`](./console.md)
- 遇到问题：[`faq.md`](./faq.md)

> ⚠️ 使用自动化工具的风险由用户自担；请勿用于代练等商业用途。
> 账号共享/自动化可能违反游戏用户协议，导致账号被封。