//! `akops doctor`：环境体检（§15）。

use std::path::Path;

use anyhow::Result;

use akops_core::config::{Workdir, detect};
use akops_core::doctor::{self, Level};

pub(crate) async fn run(wd: Workdir, mower_dir_override: Option<&Path>) -> Result<()> {
    // CLI --mower-dir 即时覆盖（不落盘）
    let overrides = detect::DetectOverrides {
        adb_path: None,
        mower_dir: mower_dir_override.map(|d| d.to_string_lossy().into_owned()),
    };
    let report = doctor::run(&wd, &overrides).await;
    let width = report.checks.iter().map(|c| c.id.len()).max().unwrap_or(5);
    for c in &report.checks {
        let mark = match c.level {
            Level::Ok => "✓",
            Level::Warn => "!",
            Level::Fail => "✗",
        };
        println!("{mark} {:<width$}  {}", c.id, c.detail, width = width);
        if let Some(hint) = &c.hint {
            println!("    ↳ {hint}");
        }
    }
    println!(
        "\n{} 项通过，{} 项警告，{} 项失败",
        report.count(Level::Ok),
        report.count(Level::Warn),
        report.count(Level::Fail)
    );
    if report.has_fail() {
        anyhow::bail!("体检存在失败项，请先修复");
    }
    Ok(())
}
