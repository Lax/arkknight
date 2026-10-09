//! `arkknight mower`：检出更新 / 回滚 / 版本（UpdatePlan，§12）。
//!
//! 避让活跃会话：有非终态会话或 daemon 存活时拒绝更新（先 drain 再更）。

use anyhow::{Result, bail};
use clap::Subcommand;

use arkknight_core::config::Workdir;
use arkknight_core::model;
use arkknight_core::update::{self, MowerPin};

#[derive(Subcommand, Debug)]
pub enum MowerCmd {
    /// 更新 mower 检出（pin commit；默认跟踪 alpha 最新）
    Update {
        /// 目标 git ref（commit/tag/分支；缺省 alpha）
        #[arg(long)]
        r#ref: Option<String>,
        /// 运行形态（决定更新后的提示文案：process | docker；默认取账号配置推断）
        #[arg(long)]
        runner: Option<String>,
    },
    /// 回滚到上次更新前的检出
    Rollback,
    /// 显示检出版本与 pin 记录
    Version,
}

pub(crate) async fn run(wd: Workdir, cmd: MowerCmd) -> Result<()> {
    let cfg = wd.load_config()?;
    let checkout = cfg.paths.mower_dir_expanded();
    match cmd {
        MowerCmd::Version => version(&wd, &checkout),
        MowerCmd::Update { r#ref, runner } => {
            let runner = match runner.as_deref() {
                Some("process") => model::RunnerKind::Process,
                Some("docker") => model::RunnerKind::Docker,
                Some(o) => bail!("runner {o:?} 不合法：process|docker"),
                None => wd
                    .load_all_accounts()?
                    .iter()
                    .find(|a| a.enabled && a.schedule.runner == model::RunnerKind::Docker)
                    .map(|_| model::RunnerKind::Docker)
                    .unwrap_or_default(),
            };
            update_cmd(&wd, &cfg, &checkout, r#ref.as_deref(), runner).await
        }
        MowerCmd::Rollback => {
            avoid_active_sessions(&wd)?;
            let (before, after) =
                update::rollback(&wd, &checkout).map_err(|e| anyhow::anyhow!("{e}"))?;
            println!("✓ 已回滚：{before} → {after}");
            println!("  DockerRunner 请重建镜像；ProcessRunner 下次会话即生效");
            Ok(())
        }
    }
}

fn version(wd: &Workdir, checkout: &std::path::Path) -> Result<()> {
    if !checkout.is_dir() {
        bail!(
            "mower 检出不存在：{}（git clone arknights-mower 后更新 paths.mower_dir）",
            checkout.display()
        );
    }
    match update::current_commit(checkout) {
        Ok(c) => println!("mower 检出：{} @ {c}", checkout.display()),
        Err(e) => println!("mower 检出：{}（{}）", checkout.display(), e),
    }
    match update::load_pin(wd) {
        Ok(Some(MowerPin {
            previous,
            current,
            ref_,
            at,
        })) => println!(
            "上次更新：{at}（{ref_}）\n  {previous} → {current}\n  回滚：arkknight mower rollback"
        ),
        _ => println!("pin 记录：无"),
    }
    Ok(())
}

async fn update_cmd(
    wd: &Workdir,
    cfg: &arkknight_core::config::AkopsConfig,
    checkout: &std::path::Path,
    target: Option<&str>,
    runner: model::RunnerKind,
) -> Result<()> {
    if !checkout.is_dir() {
        bail!(
            "mower 检出不存在：{}（git clone arknights-mower 后更新 paths.mower_dir）",
            checkout.display()
        );
    }
    avoid_active_sessions(wd)?;
    let pin = update::update(wd, checkout, target).map_err(|e| anyhow::anyhow!("{e}"))?;
    println!(
        "✓ mower 已更新：{} → {}（{}）",
        pin.previous, pin.current, pin.ref_
    );
    match runner {
        model::RunnerKind::Docker => {
            println!(
                "  DockerRunner：请用你的 Dockerfile 重建镜像并匹配 paths.docker_mower_image："
            );
            println!(
                "    docker build -t {} {}",
                cfg.paths.docker_mower_image,
                checkout.display()
            );
        }
        model::RunnerKind::Process => {
            println!("  ProcessRunner：下次会话即用新检出（活跃会话不受影响）");
        }
    }
    println!("  异常时回滚：arkknight mower rollback");
    Ok(())
}

/// 有活跃会话或 daemon 存活时拒绝（避免更新时撕裂会话/检出）。
fn avoid_active_sessions(wd: &Workdir) -> Result<()> {
    if wd.daemon_json().is_file()
        && let Ok(raw) = std::fs::read_to_string(wd.daemon_json())
        && let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw)
        && let Some(pid) = v.get("pid").and_then(|p| p.as_u64())
        && std::path::Path::new(&format!("/proc/{pid}")).exists()
    {
        bail!(
            "daemon 存活（pid={pid}）：请先 arkknight schedule pause 并 drain 会话（或停 daemon）再更新"
        );
    }
    let store = crate::commands::open_store(wd)?;
    let active = store
        .list_sessions(50)
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .into_iter()
        .filter(|s| {
            !matches!(
                s.state.as_str(),
                "finished" | "failed" | "cancelled" | "timed_out" | "switch_failed"
            )
        })
        .collect::<Vec<_>>();
    if !active.is_empty() {
        let ids = active
            .iter()
            .map(|s| format!("#{}({})", s.id, s.state))
            .collect::<Vec<_>>()
            .join(" ");
        bail!("存在非终态会话：{ids}——先 arkknight session stop 再更新");
    }
    Ok(())
}
