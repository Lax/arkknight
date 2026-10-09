# 技术评估：redroid vs waydroid（模拟器池化后端）

> 2026-10-09 · 结论见 [ADR-0003](../ai/adr/0003-模拟器池化后端维持redroid.md)：**维持 redroid，waydroid 否决**。
> 本文档保留完整对比矩阵与实测数据，供复评时使用。

## 评估场景（arkreunion M2）

- headless Linux 服务器，MAA/mower 经 adb 驱动游戏（无人工交互）
- 实例池按水位动态扩缩（目标 3+ 实例），bollard 编排
- NVIDIA GPU 本机（游戏 ARM 包需转译层）
- 游戏大版本更新 1-2 月一次，**手动触发**的镜像升级流水线
- APK 官方源：`https://ak.hypergryph.com/downloads/android_lastest`

## 对比矩阵

| 维度 | redroid | waydroid | 权重（本场景） |
|---|---|---|---|
| 架构 | AOSP 容器化（Docker，共享宿主内核 binder） | LXC + Wayland 合成器渲染（LineageOS 系） | — |
| headless | **原生**（adb-first） | 需 cage/weston-headless/WebRTC 社区拼装 | ★★★ |
| 多实例 | **N 个 Docker 容器**，API 可编排 | 无官方支持，LXC hack | ★★★ |
| 镜像/快照 | **docker commit + tag + 数据卷分层** | 整目录拷贝，无版本化原语 | ★★★ |
| adb | 容器天生映射宿主端口（`127.0.0.1:port`） | 默认无 TCP adb，进容器手工开 | ★★★ |
| NVIDIA | 可用（本机 nvidia runtime + libndk 长期验证） | **官方不支持**（nvidia-open 社区方案） | ★★ |
| ARM 转译 | libndk/libhoudini，redroid-script 免重编译注入 | 官方内置 houdini 选项 | ★ |
| 生命周期 API | Docker API（bollard 直连） | waydroid CLI / LXC 工具，无干净 API | ★★★ |
| 内核依赖 | binder_linux 模块（Android 15/16 镜像更敏感） | binder（ashmem 已废弃，memfd 替代） | ★ |
| Android 版本 | 官方镜像 11-16（16 含 2025-06 补丁级） | 官方 11/13，社区 13-16（1.6.3 起初支持 16） | ★ |
| 活跃度 | 活跃（remote-android，2025 出 15/16 镜像） | 活跃（v1.6.3，2025-05） | — |
| 适用定位 | 云/服务器批量实例 | 桌面单机图形化使用 | — |

## 本机实测数据（arknights2771）

- 容器：12GB 内存上限、8 CPU、nvidia runtime；watchdog 经 `/init.environ.rc` bind 注入；游戏数据独立 bind 卷
- 镜像分层：base 5.25GB → update 20.5GB → 带 tag 快照 35.6-35.9GB；未卷化教训 `2761-pre-vol` 66.8GB
- APK：官方链 302 → launcher API → CDN 直链 `arknights-hg-2781.apk`（1.73GB）；HEAD 405 须 GET；文件名含版本号

## APK 升级流水线（手动触发，M2 `arkreunion device upgrade-image`）

1. 避让活跃会话（`device_leases` 检查）
2. base 镜像起 staging 容器（空数据卷）
3. 实时解析 `ak.url` 短链下载 APK（不硬编码 CDN 地址）→ `adb install -r`
4. 人工 scrcpy 进游戏下载资源（游戏内 UI 不自动化，理由同 INV-1）
5. `docker commit` → `Lax/mrfz:{port}-update-{date}` → 池滚动重建（避让调度）

频率 1-2 月一次 + 手动触发，与「模拟器镜像不自动 OTA、冻结验证」原则一致。

## waydroid 复评条件

未来出现「桌面 Linux、图形交互优先、无 Docker 依赖」的新场景再议；服务器自动化场景无翻盘点。
