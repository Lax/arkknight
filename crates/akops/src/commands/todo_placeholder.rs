//! 尚未交付的子命令：按设计文档 §20 里程碑指明交付批次。
//!
//! 交付顺序（交接决策）：init/doctor → 执行器（双 Runner）→ 切号会话 →
//! 调度器 → Web 控制台。命令签名先行（CLI 形态冻结），实现按里程碑落地。

use clap::Subcommand;

fn todo(task: &str, what: &str) -> anyhow::Error {
    anyhow::anyhow!(
        "`{what}` 尚未实现——属 {task}。\n进度与交付顺序见 docs/akops-design.md §20（里程碑）"
    )
}

#[derive(Subcommand, Debug)]
pub enum SessionCmd {
    /// 手动开会话（priority=100 插队不抢占）
    Start {
        account: String,
        /// 执行器（默认 mower）
        #[arg(long, default_value = "mower")]
        executor: String,
        /// 时间片（如 90m）
        #[arg(long)]
        slice: Option<String>,
    },
    /// 停止会话
    Stop {
        /// 会话 id 或 --account
        id: Option<u64>,
        #[arg(long)]
        account: Option<String>,
    },
    /// 列出会话
    List,
    /// 跟踪会话日志
    Logs {
        id: u64,
        #[arg(short = 'f')]
        follow: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum ScheduleCmd {
    /// 查看调度器状态与队列
    Show,
    /// 暂停自动调度（运行中会话不受影响）
    Pause,
    /// 恢复自动调度
    Resume,
}

#[derive(Subcommand, Debug)]
pub enum MaaCmd {
    /// 安装 MaaCore
    Install,
    /// 更新 maa-cli / MaaCore / 资源
    Update {
        /// 仅更新资源
        #[arg(long)]
        resource_only: bool,
    },
    /// 显示版本
    Version,
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

pub(crate) fn server() -> anyhow::Result<()> {
    Err(todo(
        "M1 任务 8（控制台 v1 + API：axum + rust-embed + OpenAPI + token 鉴权）",
        "akops server",
    ))
}

pub(crate) fn provision(account: &str, device: Option<&str>) -> anyhow::Result<()> {
    let _ = (account, device);
    Err(todo(
        "M1 任务 5（provision 流程：投屏指引 + logins 记录 + 设备租约互斥）",
        "akops provision",
    ))
}

pub(crate) fn switch(
    account: &str,
    device: Option<&str>,
    timeout: Option<&str>,
) -> anyhow::Result<()> {
    let _ = (account, device, timeout);
    Err(todo(
        "M1 任务 4（MAA 集成 + 切号：唯一路径 maa run startup --batch，INV-1）",
        "akops switch",
    ))
}

pub(crate) fn session(cmd: SessionCmd) -> anyhow::Result<()> {
    let _ = cmd;
    Err(todo(
        "M1 任务 6（MowerExecutor 双 Runner：Docker|Process + 端口分配 + 深链）",
        "akops session",
    ))
}

pub(crate) fn schedule(cmd: ScheduleCmd) -> anyhow::Result<()> {
    let _ = cmd;
    Err(todo(
        "M1 任务 7（调度器 v1：时间窗 + 优先级 + 双超时 + 看门狗 + 退避）",
        "akops schedule",
    ))
}

pub(crate) fn maa(cmd: MaaCmd) -> anyhow::Result<()> {
    let _ = cmd;
    Err(todo(
        "M1 任务 4（maa-cli 包装：self update / install / 资源热更新）",
        "akops maa",
    ))
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
