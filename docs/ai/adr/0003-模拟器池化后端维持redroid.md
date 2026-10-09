# ADR-0003：模拟器池化后端维持 redroid（waydroid 评估否决）

- **状态**：已接受
- **日期**：2026-10-09
- **关联**：ADR-0001 D6（设备抽象）、`../../arkknight-design.md` §7.3（RedroidDocker 后端）、§12（模拟器镜像升级流水线）、`docs/dev/redroid-vs-waydroid.md`（完整评估）

## 背景

M2 需要按水位动态扩容安卓实例池，并要求「游戏大版本更新（1-2 月一次）可手动触发的
镜像升级流水线」（APK 官方源 `https://ak.hypergryph.com/downloads/android_lastest`）。
需评估 redroid 与 waydroid 谁作为池化后端。

## 决策

**维持 redroid 为 M2 唯一池化后端（RedroidDocker），waydroid 否决**，理由按本场景权重
（headless 服务器自动化 > 多实例池 > 镜像流水线 > adb 生态 > NVIDIA GPU）：

1. **headless**：redroid 原生（adb-first，无需显示栈）；waydroid 是 Wayland 桌面渲染优先，
   headless 只能靠 cage/weston/WebRTC 社区方案拼装。
2. **多实例池**：redroid = N 个 Docker 容器，bollard 可编程编排（arkknight 后端设计直接成立）；
   waydroid 无官方多实例，靠 LXC hack。
3. **镜像流水线**：redroid 的 `docker commit` + 镜像 tag + 数据卷分层与现有
   `Lax/mrfz:{port}-base/install/update-日期` 约定完全吻合；waydroid 只能整目录拷贝，
   无版本化原语。
4. **adb 生态**：MAA/mower 全走 adb；redroid 容器天生暴露宿主端口；waydroid 默认无
   TCP adb，需进容器手工开启。
5. **GPU**：本机 NVIDIA + redroid + libndk 已长期验证（arknights2771）；waydroid 对
   NVIDIA 官方不支持（仅 nvidia-open 社区方案）。

两者同为 Linux-only（内核 binder 依赖），不影响 arkknight 跨平台承诺——Windows 场景由
External 后端（MuMu/雷电）承接，与本 ADR 无冲突。

## APK 升级流水线（实测链路，M2 `arkknight device upgrade-image` 依据）

官方短链 302 → `launcher.hypergryph.com/game/latest/<token>` → 返回含 CDN 直链的 HTML
（如 `ak-fs.hypergryph.com/.../arknights-hg-2781.apk`，1.73GB，HEAD 405 须 GET，
文件名含版本号）。流水线手动触发：

1. 避让活跃会话（检查 `device_leases`）；
2. 基于 base 镜像起 staging 容器（空数据卷）；
3. 从 `ak.url` 短链实时解析下载 APK（不硬编码 CDN 地址）→ `adb install -r`；
4. 人工经 scrcpy 进游戏下载资源（游戏内 UI 更新不做自动化，与 INV-1 同一的腐化逻辑）；
5. 确认后 `docker commit` → `Lax/mrfz:{port}-update-{date}`，池滚动重建（避让调度）；
   数据卷不进镜像（历史教训：2761-pre-vol 66.8GB vs 卷化后 20.5GB）。

## 后果

### Android 11 数据权限豁免是 base 镜像的承载性资产（2026-10-09 事实核查）

waydroid_script 的 nodataperm hack（应用启动置 `persist.sys.nodataperm` → 数据目录 777）
针对的 Android 11 数据权限 bug 同样适用于 redroid——且**本项目的 base redroid 11 镜像出厂
已内置等价豁免**：`getenforce=Disabled`、整棵 `/data/data` 的 SELinux 上下文标记为
`HACKED`（镜像构建者的豁免标记，非本项目工具链所为）。实测该部署 4 周零 crash_events、
FUSE 共享存储属主正常（u0_a129+ext_data_rw）。

因此：
- **镜像升级流水线（§12）必须以「自带数据权限豁免的镜像」为 base**，不可换用裸
  redroid 11 镜像，否则复现应用黑屏/冻结（明日方舟为 README 点名受影响应用）
- `doctor` 增加检查项：`getenforce` 状态、抽查 `/data/data` 的 HACKED 豁免标记
- 历史症状（重启/重置/资源重下）与两因素吻合：未豁免期的黑屏冻结 + 数据卷化之前
  的镜像重建擦除数据（2761-pre-vol 时代）；现部署已由豁免+卷化+watchdog 消除


- ✅ 与现有部署资产（redroid-script、watchdog、镜像约定、双实例）零迁移成本。
- ✅ doctor 待增检查项：宿主 `binder_linux` 模块、磁盘水位（update 镜像 20-36GB/个）。
- ⚠️ redroid 宿主内核升级后需重载 binder 模块（Android 15/16 镜像对此更敏感，issue #865）。
- ⚠️ waydroid 复评条件：未来出现「桌面 Linux、图形交互优先、无 Docker」的新场景再议。
