//! `arkreunion maa`：maa-cli 包装（§12 MAA UpdatePlan）。
//!
//! 程序/资源分离：`maa self update`（CLI 本体）+ `maa update`（MaaCore+资源）；
//! 资源单独热更新 = `maa hot-update`。所有动作记 maintenance_log。

use anyhow::{Result, bail};
use clap::Subcommand;

use arkreunion_core::config::Workdir;
use arkreunion_core::store::Store;

#[derive(Subcommand, Debug)]
pub enum MaaCmd {
    /// 安装 MaaCore 与资源
    Install,
    /// 更新：maa-cli 本体 + MaaCore + 资源
    Update {
        /// 仅热更新资源（maa hot-update）
        #[arg(long)]
        resource_only: bool,
    },
    /// 显示 maa-cli / MaaCore 版本
    Version,
}

pub(crate) async fn run(wd: Workdir, cmd: MaaCmd) -> Result<()> {
    match cmd {
        MaaCmd::Version => {
            run_maa(&["version"], true).await?;
            Ok(())
        }
        MaaCmd::Install => {
            run_maa(&["install"], false).await?;
            record(&wd, "install", None, true, None)?;
            println!("✓ MaaCore 与资源安装流程结束");
            Ok(())
        }
        MaaCmd::Update { resource_only } => {
            if resource_only {
                run_maa(&["hot-update"], false).await?;
                record(&wd, "update_resource", None, true, None)?;
                println!("✓ MAA 资源热更新完成");
            } else {
                println!("→ 更新 maa-cli 本体…");
                // self update 失败不阻断 MaaCore 更新（CLI 更新期间自身被替换属正常现象）
                match run_maa(&["self", "update"], false).await {
                    Ok(()) => println!("✓ maa-cli 已是最新"),
                    Err(e) => println!("! maa self update：{e}"),
                }
                println!("→ 更新 MaaCore 与资源…");
                run_maa(&["update"], false).await?;
                record(&wd, "update", None, true, None)?;
                println!("✓ MAA 更新完成");
            }
            println!("提示：更新后建议运行 arkreunion doctor 复检");
            Ok(())
        }
    }
}

/// 运行 maa 子命令。`quiet=true` 捕获输出并回显（子进程 stdout 必须有人读，
/// 否则写端 EPIPE 会让 maa-cli panic）；否则继承 stdio（用户直接看到进度/交互）。
async fn run_maa(args: &[&str], quiet: bool) -> Result<()> {
    let mut cmd = tokio::process::Command::new("maa");
    cmd.args(args);
    if quiet {
        cmd.stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        let out = cmd
            .output()
            .await
            .map_err(|e| anyhow::anyhow!("spawn maa 失败（PATH 中无 maa？）：{e}"))?;
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        if !out.status.success() {
            bail!(
                "maa {} 退出码 {:?}：{}",
                args.join(" "),
                out.status.code(),
                if stderr.trim().is_empty() {
                    stdout.trim()
                } else {
                    stderr.trim()
                }
            );
        }
        if !stdout.trim().is_empty() {
            print!("{stdout}");
        }
        if !stderr.trim().is_empty() {
            eprint!("{stderr}");
        }
        Ok(())
    } else {
        cmd.stdin(std::process::Stdio::inherit());
        let status = cmd
            .status()
            .await
            .map_err(|e| anyhow::anyhow!("spawn maa 失败（PATH 中无 maa？）：{e}"))?;
        if !status.success() {
            bail!("maa {} 退出码 {:?}", args.join(" "), status.code());
        }
        Ok(())
    }
}

fn record(
    wd: &Workdir,
    action: &str,
    from: Option<&str>,
    ok: bool,
    detail: Option<&str>,
) -> Result<()> {
    // maintenance_log 允许失败（db 未初始化等场景不阻断更新本身）
    match Store::open(&wd.db_path()) {
        Ok(store) => {
            if let Err(e) = store.insert_maintenance("maa", action, from, None, ok, detail) {
                tracing::warn!("记录维护日志失败：{e}");
            }
        }
        Err(e) => tracing::warn!("记录维护日志失败（无法打开数据库）：{e}"),
    }
    Ok(())
}
