//! 尚未交付的子命令：按设计文档 §20 里程碑指明交付批次。
//!
//! 已实装：init/doctor/status/account/device/completions（任务 1/2）、
//! switch/provision/maa（任务 4/5）、session/mower（任务 6，含 DockerRunner）。
//! 调度器（任务 7）与 server 骨架（任务 8 前半）已实装。
//! 剩余占位：export/import（任务 9）、migrate（随 store 迁移需求落地后移除占位）。

fn todo(task: &str, what: &str) -> anyhow::Error {
    anyhow::anyhow!(
        "`{what}` 尚未实现——属 {task}。\n进度与交付顺序见 docs/arkknight-design.md §20（里程碑）"
    )
}

pub(crate) fn export() -> anyhow::Result<()> {
    Err(todo(
        "M1 任务 9（导出：zip{manifest+配置树+db}，--redact-secrets 脱敏，INV-2）",
        "arkknight export",
    ))
}

pub(crate) fn import() -> anyhow::Result<()> {
    Err(todo(
        "M1 任务 9（导入：manifest 校验 + schema_version 拒绝高版本）",
        "arkknight import",
    ))
}

pub(crate) fn migrate() -> anyhow::Result<()> {
    Err(todo(
        "M1 任务 3/7（SQLite 运行态落地后才有 schema 迁移）",
        "arkknight migrate",
    ))
}
