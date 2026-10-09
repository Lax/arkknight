//! `arkknight status`：只读总览（§15）。

use anyhow::Result;
use serde_json::Value;

use arkknight_core::config::Workdir;

pub(crate) async fn run(wd: Workdir) -> Result<()> {
    let cfg = wd.load_config()?;
    println!("工作目录  {}", wd.root.display());
    println!(
        "调度      时区 {}，游戏日界 {}，默认时间片 {}（调度器：M1 任务 7 交付）",
        cfg.scheduler.timezone, cfg.scheduler.game_day_boundary, cfg.scheduler.default_slice
    );

    let accounts = wd.load_all_accounts()?;
    println!("\n账号（{}）：", accounts.len());
    if accounts.is_empty() {
        println!("  （无——arkknight account add <id> 注册）");
    }
    for a in &accounts {
        let windows = a
            .schedule
            .windows
            .iter()
            .map(|w| format!("{}-{}:{:?}", w.start, w.end, w.executor))
            .collect::<Vec<_>>()
            .join(" ");
        println!(
            "  {} {:<14} {:<9} {}  窗口[{}]",
            if a.enabled { "✓" } else { "×" },
            a.key,
            format!("{:?}", a.server).to_lowercase(),
            a.account_name,
            if windows.is_empty() {
                "无（不参与自动调度）"
            } else {
                &windows
            }
        );
    }

    let devices = wd.load_all_devices()?;
    println!("\n设备（{}）：", devices.len());
    if devices.is_empty() {
        println!("  （无——arkknight device add <name> --host-adb <addr> 注册）");
    }
    for d in &devices {
        println!(
            "  {} {:<14} host={}",
            if d.backend == arkknight_core::model::DeviceBackendKind::External {
                "✓"
            } else {
                "r"
            },
            d.name,
            d.connection.host_adb
        );
    }

    // daemon 信息（存在则展示；存活判定在 server 实现后补心跳校验）
    if let Ok(raw) = std::fs::read_to_string(wd.daemon_json()) {
        if let Ok(v) = serde_json::from_str::<Value>(&raw) {
            println!(
                "\ndaemon     pid={} port={} started_at={}",
                v.get("pid").and_then(Value::as_u64).unwrap_or(0),
                v.get("port").and_then(Value::as_u64).unwrap_or(0),
                v.get("started_at").and_then(Value::as_str).unwrap_or("?")
            );
        }
    } else {
        println!("\ndaemon     未运行（arkknight server 启动，M1 任务 8 交付）");
    }
    println!("会话/队列  调度器与 SQLite 运行态属 M1 任务 3/7 交付");
    Ok(())
}
