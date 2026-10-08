//! 账号切换（§9）：**INV-1 唯一实现入口，M1 任务 4 交付。**
//!
//! # 硬性不变量（违反即拒改）
//!
//! 唯一路径：`run_switch()` → `maa run startup -p default --batch`
//! （StartUp 任务 + `account_name` 参数，MAA 官方能力）。
//!
//! **禁止**出现：登录界面截图识别、坐标点击、OCR、输入法模拟等任何自研
//! 登录自动化代码。MAA 不可用/切号失败 → 会话 `SwitchFailed` + 告警，不降级。
//!
//! # 标准流程（§9.1）
//!
//! ```text
//! 持有设备租约
//!   ├─ 设备上有活跃会话 → drain（优雅停止）
//!   ├─ adb shell am force-stop <包名（按 server）>
//!   ├─ 物化器改写 MAA profile 的 connection.address（宿主/容器视角）
//!   └─ maa run startup -p default --batch
//!        ├─ 成功（退出码 0 且任务链完成）→ 移交执行器
//!        └─ 失败 → 重试 ≤ max_switch_retries（指数退避）→ SwitchFailed
//! ```
//!
//! 亲和前提（§9.2）：仅对「该账号曾在此设备登录过」（SQLite `logins`）的
//! 设备有效；首次使用走 `provision` 人工预置（§9.3）。
