//! `arkknight provision`：账号预置引导（§9.3）——每账号每设备一次性人工登录。

use anyhow::{Context, Result};
use std::io::Write;

use arkknight_core::config::Workdir;
use arkknight_core::device::build_backend;
use arkknight_core::lock::WorkdirGuard;
use arkknight_core::store::Store;
use std::sync::Arc;

use crate::commands::resolve_device;

pub(crate) async fn run(
    wd: Workdir,
    _guard: WorkdirGuard,
    store: Arc<Store>,
    account_key: String,
    device: Option<String>,
) -> Result<()> {
    let cfg = wd.load_config()?;
    let account = wd.load_account(&account_key)?;
    let dev = resolve_device(&wd, device.as_deref())?;
    let holder = format!("provision:{}", account.key);

    // 设备在线 + 游戏安装检查
    let backend = build_backend(&dev, &cfg.paths.adb_path_expanded())?;
    let health = backend.health().await.context("设备不在线，无法预置")?;
    if !health.reachable {
        anyhow::bail!("设备 {} 不在线（adb 状态：{}）", dev.name, health.detail);
    }
    let pkgs = backend.detect_game_packages().await.unwrap_or_default();
    let target_pkg = account.server.game_package();
    if !pkgs.iter().any(|p| p == target_pkg) {
        println!(
            "⚠ 未检测到该账号服务器的游戏包（{}）——请先安装对应服务器客户端",
            target_pkg
        );
    }

    // 设备租约（与调度互斥，§9.3 第 5 点）
    store
        .acquire_lease(&dev.name, &holder)
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let released = LeaseOnDrop {
        store: store.clone(),
        device: dev.name.clone(),
        holder: holder.clone(),
    };

    println!();
    println!("=== 账号预置：{} @ {} ===", account.key, dev.name);
    println!(
        "目标：在该设备上人工登录一次账号「{}」（{}）",
        account.display_name, account.account_name
    );
    println!();
    println!("投屏/操作指引：");
    println!(
        "  1. 投屏：scrcpy -s {serial}   （或使用 ws-scrcpy 等任意投屏工具）",
        serial = dev.connection.host_adb
    );
    println!(
        "  2. 拉起游戏：adb -s {serial} shell monkey -p {pkg} -c android.intent.category.LAUNCHER 1",
        serial = dev.connection.host_adb,
        pkg = target_pkg
    );
    println!("  3. 登录目标账号，并勾选「记住密码/快速登录」（MAA 靠快速登录列表选号）");
    println!("  4. 登录完成后回到这里按回车");
    println!();

    wait_for_enter().context("读取确认输入失败")?;

    // UID 录入（§9.4：切号后 OCR 核验防串数据；可跳过但 doctor 会告警）
    print!("请输入该账号的游戏内 UID（主界面点头像可见；回车跳过）> ");
    std::io::stdout().flush().ok();
    let mut uid = String::new();
    std::io::stdin()
        .read_line(&mut uid)
        .context("读取 UID 失败")?;
    let uid = uid.trim().to_string();
    if !uid.is_empty() {
        if !uid.chars().all(|c| c.is_ascii_digit()) {
            anyhow::bail!("UID 须为纯数字：{uid:?}");
        }
        let mut acc = wd.load_account(&account.key).map_err(anyhow::Error::from)?;
        acc.uid = Some(uid.clone());
        wd.save_account(&acc).map_err(anyhow::Error::from)?;
        println!("✓ 已记录 UID {uid}（切号后将自动核验，§9.4）");
    } else {
        println!("! 未记录 UID：切号后不做核验（doctor 将提示串数据风险）");
    }

    // 复验在线并记录 logins
    let health = backend.health().await.context("确认时设备已离线")?;
    if !health.reachable {
        anyhow::bail!("设备 {} 已离线，预置未记录；请重试", dev.name);
    }
    store
        .record_login(&account.key, &dev.name, "provisioned")
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    println!(
        "✓ 已记录 logins({} @ {} = provisioned)",
        account.key, dev.name
    );
    println!(
        "  下一步：arkknight switch {} --device {}   # 验证自动切号",
        account.key, dev.name
    );

    drop(released);
    Ok(())
}

struct LeaseOnDrop {
    store: Arc<Store>,
    device: String,
    holder: String,
}

impl Drop for LeaseOnDrop {
    fn drop(&mut self) {
        let _ = self.store.release_lease(&self.device, &self.holder);
    }
}

/// 等待用户回车（阻塞 stdin；CLI 交互场景可接受）。
fn wait_for_enter() -> Result<()> {
    use std::io::Write;
    print!("完成登录后按回车确认（输入 q 回车取消）> ");
    std::io::stdout().flush().ok();
    let mut line = String::new();
    std::io::stdin()
        .read_line(&mut line)
        .context("读取 stdin 失败")?;
    let t = line.trim().to_lowercase();
    if t == "q" || t == "abort" || t == "cancel" {
        anyhow::bail!("已取消预置（未记录 logins）");
    }
    Ok(())
}
