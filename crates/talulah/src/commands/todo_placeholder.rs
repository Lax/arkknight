//! 尚未交付的子命令：按设计文档 §20 里程碑指明交付批次。
//!
//! 已实装：init/doctor/status/account/device/completions（任务 1/2）、
//! switch/provision/maa（任务 4/5）、session（任务 6 前半，ProcessRunner）。
//! 剩余占位：mower update（任务 6 后半）、export/import（任务 9）、
//! migrate（随 store 交付后移除占位）。调度器（任务 7）与 server 骨架（任务 8 前半）已实装。

use clap::Subcommand;

fn todo(task: &str, what: &str) -> anyhow::Error {
    anyhow::anyhow!(
        "`{what}` 尚未实现——属 {task}。\n进度与交付顺序见 docs/akops-design.md §20（里程碑）"
    )
}

#[derive(Subcommand, Debug)]
pub enum MowerCmd {
    /// 更新 mower 检出（pin ref，支持回滚）
    Update {
        /// 目标 git ref（默认跟踪 alpha 最新）
        #[arg(long)]
        r#ref: Option<String>,
        /// 运行形态（默认配置推断）
        #[arg(long)]
        runner: Option<String>,
    },
    /// 显示检出版本
    Version,
}

pub(crate) fn mower(cmd: MowerCmd) -> anyhow::Result<()> {
    let _ = cmd;
    Err(todo(
        "M1 任务 6（mower UpdatePlan：git pin + 回滚 + 避让活跃会话）",
        "akops mower",
    ))
}

pub(crate) fn export() -> anyhow::Result<()> {
    Err(todo(
        "M1 任务 9（导出：zip{manifest+配置树+db}，--redact-secrets 脱敏，INV-2）",
        "akops export",
    ))
}

pub(crate) fn import() -> anyhow::Result<()> {
    Err(todo(
        "M1 任务 9（导入：manifest 校验 + schema_version 拒绝高版本）",
        "akops import",
    ))
}

pub(crate) fn migrate() -> anyhow::Result<()> {
    Err(todo(
        "M1 任务 3/7（SQLite 运行态落地后才有 schema 迁移）",
        "akops migrate",
    ))
}
