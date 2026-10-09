//! `arkreunion account`：账号管理（§15；M1 范围：add/list/show/remove/enable/disable，
//! import-mower/import-maa 属 §11.6 反向导入器，M1 末交付）。

use anyhow::{Result, bail};
use clap::Subcommand;

use arkreunion_core::config::Workdir;
use arkreunion_core::model::{
    Account, AccountSchedule, ScheduledExecutor, Server, TimeWindow, account::validate_slug,
};

#[derive(Subcommand, Debug)]
pub enum AccountCmd {
    /// 注册账号（写入 accounts/<id>/account.toml 并创建 maa/mower 目录骨架）
    Add {
        /// 账号 id（目录名，slug）
        id: String,
        /// 服务器类型
        #[arg(long)]
        server: Server,
        /// MAA 切号匹配串：官服=打码手机号片段（如 123****8901），B服=昵称
        #[arg(long)]
        account_name: String,
        /// 游戏 UID（切号后 OCR 核验防串数据，可后补）
        #[arg(long)]
        uid: Option<String>,
        /// 展示名（默认= id）
        #[arg(long)]
        display_name: Option<String>,
        /// 队列优先级 0-100（默认 50）
        #[arg(long, default_value_t = 50)]
        priority: u8,
        /// 时间片（覆盖全局默认，如 90m）
        #[arg(long)]
        slice: Option<String>,
        /// 时间窗 `HH:MM-HH:MM:mower|maa`，可重复
        #[arg(long = "window")]
        windows: Vec<String>,
    },
    /// 列出账号
    List,
    /// 查看账号详情
    Show { id: String },
    /// 删除账号（含其 maa/mower bundle，不可恢复）
    Remove {
        id: String,
        /// 跳过确认
        #[arg(long)]
        yes: bool,
    },
    /// 启用调度
    Enable { id: String },
    /// 停用调度
    Disable { id: String },
}

pub(crate) async fn run(wd: Workdir, cmd: AccountCmd) -> Result<()> {
    // 写类子命令须持有单写者锁（D7）；读类直接走
    let _guard = match cmd {
        AccountCmd::List | AccountCmd::Show { .. } => None,
        _ => Some(crate::commands::write_lock(&wd)?),
    };
    match cmd {
        AccountCmd::Add {
            id,
            server,
            account_name,
            uid,
            display_name,
            priority,
            slice,
            windows,
        } => {
            validate_slug(&id, "账号 id").map_err(anyhow::Error::from)?;
            if wd.account_file(&id).exists() {
                bail!("账号 {id} 已存在");
            }
            let mut windows_parsed = Vec::new();
            for w in &windows {
                windows_parsed.push(parse_window(w)?);
            }
            let slice = match slice.as_deref() {
                Some(s) => {
                    Some(arkreunion_core::config::HumanDuration::parse(s).map_err(anyhow::Error::msg)?)
                }
                None => None,
            };
            let acc = Account {
                id: id.clone(),
                display_name: display_name.unwrap_or_else(|| id.clone()),
                server,
                account_name,
                uid,
                enabled: true,
                schedule: AccountSchedule {
                    windows: windows_parsed,
                    priority,
                    slice,
                },
                provisioned_on: vec![],
            };
            acc.validate().map_err(anyhow::Error::from)?;
            // account_name 跨账号唯一性提前校验（最终以 MAA 运行结果为准，§9.1）
            wd.save_account(&acc).map_err(anyhow::Error::from)?;
            let dups = wd.account_name_duplicates().map_err(anyhow::Error::from)?;
            if let Some((name, ids)) = dups.iter().find(|(_, ids)| ids.contains(&id)) {
                wd.remove_account(&id).map_err(anyhow::Error::from)?;
                bail!(
                    "account_name {name:?} 已被账号 {} 使用：切号匹配串须唯一",
                    ids.join(",")
                );
            }
            println!("✓ 账号 {id} 已注册：{}", wd.account_file(&id).display());
            println!("  下一步：arkreunion provision {id} --device <name>（人工登录一次，切号前提）");
            Ok(())
        }
        AccountCmd::List => {
            for acc in wd.load_all_accounts().map_err(anyhow::Error::from)? {
                println!(
                    "{}\t{}\t{:?}\t{}\t{}",
                    acc.id,
                    acc.display_name,
                    acc.server,
                    acc.account_name,
                    if acc.enabled { "enabled" } else { "disabled" }
                );
            }
            Ok(())
        }
        AccountCmd::Show { id } => {
            // 先 load 完成校验，再原样展示文件
            wd.load_account(&id).map_err(anyhow::Error::from)?;
            print!(
                "{}",
                std::fs::read_to_string(wd.account_file(&id)).map_err(|e| anyhow::anyhow!(e))?
            );
            Ok(())
        }
        AccountCmd::Remove { id, yes } => {
            if !yes {
                bail!("删除账号 {id} 将连同其 maa/mower bundle 一起移除；确认请加 --yes");
            }
            wd.remove_account(&id).map_err(anyhow::Error::from)?;
            println!("✓ 账号 {id} 已删除");
            Ok(())
        }
        AccountCmd::Enable { id } => toggle(&wd, &id, true),
        AccountCmd::Disable { id } => toggle(&wd, &id, false),
    }
}

fn toggle(wd: &Workdir, id: &str, enabled: bool) -> Result<()> {
    let mut acc = wd.load_account(id).map_err(anyhow::Error::from)?;
    acc.enabled = enabled;
    wd.save_account(&acc).map_err(anyhow::Error::from)?;
    println!("✓ 账号 {id} 已{}", if enabled { "启用" } else { "停用" });
    Ok(())
}

/// 解析 `HH:MM-HH:MM:mower|maa[:task]`（task 仅 maa 窗口需要）。
fn parse_window(s: &str) -> Result<TimeWindow> {
    let toks: Vec<&str> = s.split(['-', ':']).collect();
    if toks.len() != 5 && toks.len() != 6 {
        bail!("时间窗 {s:?} 不合法：应为 HH:MM-HH:MM:mower|maa[:task]");
    }
    let executor = match toks[4] {
        "mower" => ScheduledExecutor::Mower,
        "maa" => ScheduledExecutor::Maa,
        other => bail!("时间窗执行器 {other:?} 不合法：mower|maa"),
    };
    let task = toks.get(5).map(|t| t.to_string());
    if executor == ScheduledExecutor::Mower && task.is_some() {
        bail!("task 仅对 maa 窗口有效");
    }
    Ok(TimeWindow {
        start: format!("{}:{}", toks[0], toks[1]),
        end: format!("{}:{}", toks[2], toks[3]),
        executor,
        task,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_parse() {
        let w = parse_window("08:00-12:00:mower").unwrap();
        assert_eq!(w.start, "08:00");
        assert_eq!(w.end, "12:00");
        assert_eq!(w.executor, ScheduledExecutor::Mower);
        assert_eq!(w.task, None);
        let w = parse_window("20:30-22:00:maa:roguelike").unwrap();
        assert_eq!(w.executor, ScheduledExecutor::Maa);
        assert_eq!(w.task.as_deref(), Some("roguelike"));
        assert!(parse_window("8:00-12:00").is_err());
        assert!(parse_window("08:00-12:00:xyz").is_err());
        assert!(parse_window("08:00-12:00:mower:sometask").is_err());
    }
}
