//! `arkreunion switch`：手动切号（§9.1；INV-1 唯一路径）。

use std::time::Duration;

use anyhow::{Context, Result};

use arkreunion_core::config::{AkopsConfig, HumanDuration, Workdir};
use arkreunion_core::lock::WorkdirGuard;
use arkreunion_core::switch::{SwitchCtx, run_switch};

use crate::commands::{open_store, recover_orphans, resolve_device};

pub(crate) async fn run(
    wd: Workdir,
    _guard: WorkdirGuard,
    account_key: String,
    device: Option<String>,
    timeout: Option<String>,
) -> Result<()> {
    let cfg: AkopsConfig = wd.load_config()?;
    let store = open_store(&wd)?;
    recover_orphans(&store);

    let account = wd.load_account(&account_key)?;
    let dev = resolve_device(&wd, device.as_deref())?;

    let mut ctx = SwitchCtx::new(&wd, &cfg, store, account, dev);
    if let Some(t) = timeout {
        let d: HumanDuration = t.parse().map_err(anyhow::Error::msg)?;
        ctx.timeout = Duration::from_millis(d.0.as_millis() as u64);
    }

    println!(
        "切号 {} → {}（MAA 官方能力：maa run startup --batch）…",
        ctx.account.key, ctx.device.name
    );
    let outcome = run_switch(&ctx).await.context("切号失败")?;
    if outcome.ok {
        println!(
            "✓ 切号成功：{} 次尝试，耗时 {:.1}s",
            outcome.attempts,
            outcome.duration_ms as f64 / 1000.0
        );
        Ok(())
    } else {
        anyhow::bail!(
            "切号失败（{} 次尝试耗尽）：{}\n提示：MAA 资源可能落后于登录界面改版，运行 `arkreunion maa update` 后重试",
            outcome.attempts,
            outcome.error.as_deref().unwrap_or("未知错误")
        );
    }
}
