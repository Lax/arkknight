//! `arkreunion schedule`：调度器开关（§15）。daemon 存活时经本地 API 生效（D7）。

use anyhow::{Result, bail};
use clap::Subcommand;

use arkreunion_core::config::Workdir;

use crate::commands::server::{daemon_port, http_call};

#[derive(Subcommand, Debug)]
pub enum ScheduleCmd {
    /// 查看调度器状态与队列
    Show,
    /// 暂停自动调度（运行中会话不受影响）
    Pause,
    /// 恢复自动调度
    Resume,
}

pub(crate) async fn run(wd: Workdir, cmd: ScheduleCmd) -> Result<()> {
    match cmd {
        ScheduleCmd::Show => show(&wd).await,
        ScheduleCmd::Pause => toggle(&wd, true).await,
        ScheduleCmd::Resume => toggle(&wd, false).await,
    }
}

async fn toggle(wd: &Workdir, pause: bool) -> Result<()> {
    let Some(port) = daemon_port(wd) else {
        bail!(
            "daemon 未运行（{}）：请先 arkreunion server 启动，或直接停止 daemon 后由 CLI 直写",
            wd.daemon_json().display()
        );
    };
    let cfg = wd.load_config().map_err(anyhow::Error::from)?;
    let path = if pause {
        "/api/schedule/pause"
    } else {
        "/api/schedule/resume"
    };
    http_call(port, "POST", path, &cfg.server.token).await?;
    println!("✓ 调度器已{}", if pause { "暂停" } else { "恢复" });
    Ok(())
}

async fn show(wd: &Workdir) -> Result<()> {
    let cfg = wd.load_config().map_err(anyhow::Error::from)?;
    if let Some(port) = daemon_port(wd) {
        match http_call(port, "GET", "/api/status", &cfg.server.token).await {
            Ok(body) => {
                let v: serde_json::Value =
                    serde_json::from_str(&body).unwrap_or(serde_json::json!({}));
                println!("daemon       127.0.0.1:{port}");
                println!(
                    "paused       {}",
                    v.get("paused").and_then(|x| x.as_bool()).unwrap_or(false)
                );
                println!(
                    "活跃会话     {:?}",
                    v.get("active_session").and_then(|x| x.as_u64())
                );
                if let Some(accounts) = v.get("accounts").and_then(|x| x.as_array()) {
                    println!("账号（{}）：", accounts.len());
                    for a in accounts {
                        println!(
                            "  {} {:<12} priority={} 窗口 {:?}",
                            !a.get("enabled").and_then(|x| x.as_bool()).unwrap_or(false),
                            a.get("id").and_then(|x| x.as_str()).unwrap_or("?"),
                            a.get("priority").and_then(|x| x.as_u64()).unwrap_or(0),
                            a.get("windows").cloned().unwrap_or_default(),
                        );
                    }
                }
                return Ok(());
            }
            Err(e) => println!("! daemon API 不可达（{e}），回退本地读取"),
        }
    }
    // 本地兜底（daemon 未运行）：配置 + 最近会话
    println!("daemon       未运行（arkreunion server 启动）");
    println!(
        "调度配置     时区 {}，日界 {}，默认时间片 {}，drain grace {}",
        cfg.scheduler.timezone,
        cfg.scheduler.game_day_boundary,
        cfg.scheduler.default_slice,
        cfg.scheduler.drain_grace
    );
    let store =
        arkreunion_core::store::Store::open(&wd.db_path()).map_err(|e| anyhow::anyhow!("{e}"))?;
    let rows = store
        .list_sessions(10)
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    if rows.is_empty() {
        println!("最近会话     （无）");
    } else {
        println!("最近会话：");
        for r in rows {
            println!(
                "  #{} {} {} {} outcome={:?}",
                r.id, r.state, r.account_key, r.device_name, r.outcome
            );
        }
    }
    Ok(())
}
