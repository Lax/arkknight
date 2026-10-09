# 技术调研：Android 设备侧 Agent 方案（arkknight 编排服务层）

> 2026-10-09 · 状态：**调研完成，待决策**（是否立项自研 arkknight-agent）
> 动机：像 mower 推送 scrcpy-server/MaaTouch/DroidCast 那样，把编排所需的服务装进 Android，
> 使 arkknight 的会话守护/设备指标/装包能力不依赖「能改镜像」（watchdog 现在只能注入自制 redroid 镜像，
> MuMu/雷电/BlueStacks 等第三方模拟器上缺失）。

## 一、业界设备侧 Agent 模式盘点

通用原理链：`adb push` 到 `/data/local/tmp`（shell_data_file 上下文，shell 域可 exec）→
`app_process`（Java/dex）或直接 exec（native 静态二进制）以 shell uid 启动 → 通过本地 socket /
adb forward|reverse 暴露服务 → 主机消费。无需 root；个别厂商 ROM 的 SELinux 策略是主要变数。

| 方案 | 形态 | 能力 | 现状（2025-2026） | 对 arkknight 的适用性 |
|---|---|---|---|---|
| scrcpy-server | jar via app_process | 视频（H.264/AV1）+ 控制注入 | Genymobile 持续维护，Apache-2.0 | 控制面属执行器领域；arkknight 不碰 |
| MaaTouch | native 二进制 | 多点触控注入 | MAA 内置分发 | 同上（mower 已用作触控后端） |
| DroidCast | APK（mower 经 `CLASSPATH=<apk> app_process` 直跑 `Main` 类 + adb forward；上游亦支持 am broadcast） | PNG 截图流 | mower 内置 vendor（1.3.0，SHA-256 校验锚定上游 commit c779c66） | **可复用**：控制台设备画面（可选）。⚠️ 实证副作用：清单含 `LAUNCHER`+`MainActivity`，安装后在模拟器桌面留下 "DroidCast" 图标且 mower 从不卸载（`com.rayworks.droidcast`，仅当 `screenshot_backend == "droidcast"` 时安装） |
| atx-agent / uiautomator2 | Go 二进制 + UiAutomator HTTP | UI 自动化全家桶 | uiautomator2 3.x 已转向 appium 风格 server，atx-agent 事实遗留；ABI 兼容历史问题多 | 不复用（功能越界 + 维护风险） |
| sonic-android-agent | Java/Kotlin agent | 远控/自动化测试平台 | 活跃，但绑定 Sonic 平台 | 太重，不绑定 |
| DeviceFarmer/STF | 宿主 provider + web | 设备农场管理 | 维护中 | 架构参考（租约/预约/远控），非 agent 复用对象 |

## 二、mower 的实际做法（本机源码证据）

`arknights_mower/utils/device/`：三类并存——
1. **设备侧 agent**：`scrcpy/`（截图+控制）、`maatouch/`（触控）、`droidcast.py`（APK 安装
   `com.rayworks.droidcast`，版本化安装 + 占有权校验 + HTTP PNG）
2. **厂商专有适配**：`mumu12ipc`（MuMu 12 IPC 截屏）、`ldplayer_capture/discovery`、
   `bluestacks_air/discovery`、`genymotion.py`、`avd.py`
3. **兜底**：`adb_client`（`screencap -p` / `input`）

**arkknight 学什么**：跨模拟器的服务统一靠「推 agent」；哪台设备用哪条通道由能力探测决定，永远有
adb 兜底。**不学什么**：厂商深度适配矩阵是执行器（mower/MAA）的领域，arkknight 不背。

## 三、arkknight 的真实痛点与 agent 能力清单（按价值排序）

| # | 痛点 | agent 能力 | 价值 |
|---|---|---|---|
| 1 | watchdog（崩溃日志/孤儿清理/RSS 守护）经 init.rc bind 注入，**仅自制 redroid 可用**；第三方模拟器上会话守护缺失 | 守护进程：crash log 采集、孤儿清理、RSS 阈值动作 | ★★★（第三方模拟器会话质量的前提） |
| 2 | M2 水位准入/亲和/控制台需要**每设备指标**（游戏 RSS、帧率、电量），现在无统一来源 | `/proc`/`dumpsys` 定期采样上报 | ★★★（M2 水位准入的数据面） |
| 3 | `health()` 仅 adb get-state，信息薄 | 心跳 + 版本/ABI/前台应用上报 | ★★ |
| 4 | 控制台「设备页」无实时画面 | 复用 DroidCast | ★（可选） |
| 5 | 升级流水线 `adb install` 已通用 | 安装进度/校验增益 | ★ |

**不做**：截屏/触控控制面（mower/MAA 领域，避免重复造轮子）；UI 自动化（INV-1 边界 + 腐化逻辑同源）。

## 四、复用 vs 自研

**形态光谱与决策框架**（2026-10-09 事实核查 + 复议；取代早先「必须无痕」的绝对表述）：

| 形态 | 桌面图标 | 应用列表 | 保活/自启 | 成本 |
|---|---|---|---|---|
| A. 无痕：`/data/local/tmp` + exec/`app_process` | 无 | 不可见 | **无**（进程被杀即消失，宿主重启即无） | push+chmod，最轻 |
| B. 半无痕 APK（无 LAUNCHER） | 无 | 可见、可卸载 | 有 Context/前台服务/START_STICKY/BOOT_COMPLETED | `pm install` + 固定签名 |
| C. 桌面 APK（带 LAUNCHER，DroidCast 即此类） | 有 | 可见 | 同 B | 图标=状态可见性，但污染桌面 |

决策规则（能力驱动，非洁癖驱动）：
- **A 为默认起点**（心跳/指标/轻量守护够用且 bootstrap 最轻）——这是排期选择，不是禁令
- 看门狗需要「被杀自愈、开机自启」时（app_process 进程无任何拉活机制），升 **B**：
  前台服务 + BOOT_COMPLETED 是标准解；固定自签签名避免 DroidCast 式签名冲突
- **C** 仅当把「桌面可见的守护状态」当 feature（可点开看状态/停止）
- **透明度约束（替代无痕禁令）**：无论何种形态，`doctor`/`status` 必须如实报告 agent 的
  存在/版本/运行态，并提供显式卸载命令；**无痕 ≠ 反检测**——arkknight 不做行为伪装（R4 诚实原则），
  无痕仅指不占用户桌面、应用列表状态如实可控

自研 `arkknight-agent` v0（建议 Go：`GOOS=android` 免 cgo 交叉编译 arm64/x86_64 最省事；Rust+NDK 亦可）：
- 功能面：`POST /heartbeat`、`GET /metrics`、`POST /app/install|launch|force-stop`、
  watchdog 守护模式、崩溃日志拉取；HTTP 绑定设备内 localhost，经 `adb forward` 暴露宿主
- 体积目标 <5MB/ABI；版本握手（arkknight ↔ agent），mismatch 自动重推（吸取 atx-agent「recover failed」教训）
- 部署形态：redroid 可烘焙 init.rc 自启（替代 watchdog bind hack）；其他设备每次会话经 adb 启动

## 五、架构落点（INV-3 兼容）

- `arkknight-core/src/agent/`：`DeviceAgent` trait（deploy/heartbeat/metrics/app_ops/watchdog）
- `DeviceBackend` 挂可选能力；`devices/<name>.toml` 增 `agent = true|false`（默认 false）
- **纯 adb 永远是默认与降级路径**——agent 缺席时所有功能降级（watchdog 不可用→仅 redroid、
  指标→dumpsys 轮询），不引入新的失败模式
- bootstrap：`adb push + chmod + 执行`（shell 域），失败时输出 SELinux 诊断提示并降级

## 六、风险

1. 厂商 ROM SELinux 限制 `/data/local/tmp` exec（真机少数情况）→ 探测 + 明确降级提示
2. agent 版本漂移/僵尸进程 → 版本握手 + 会话启动时校验重启
3. 安全：agent 只绑设备内 localhost + adb forward，token 握手；不监听网络接口
4. 维护腐化：功能面锁死在编排服务，控制面需求一律转执行器

## 七、分期建议（待决策）

- **Phase A（低成本 spike，可并入 M2）**：纯 adb `dumpsys` 指标轮询 + 可选 DroidCast 设备画面，
  验证控制台/水位对指标的真实需求
- **Phase B**：arkknight-agent v0（心跳/指标/watchdog 守护），redroid + 一个第三方模拟器（如 MuMu）双端验证
- **Phase C（可选）**：装包流水线接 agent、DroidCast 画面整合进控制台
