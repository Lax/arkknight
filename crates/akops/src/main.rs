//! akops CLI：rails 式子命令（设计文档 §15）。
//!
//! 全局参数：`--workdir`（默认 cwd 向上探测 `akops.toml`）、`--maa-dir`/`--mower-dir`
//! （覆盖 `[paths]`）、`--config`（替代配置文件）、`-v`（提升日志级别）。

mod commands;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{ArgAction, CommandFactory, Parser, Subcommand};
use clap_complete::{Shell, generate};

use akops_core::config::Workdir;

/// 全局参数。
#[derive(Parser, Debug)]
#[command(
    name = "akops",
    version,
    about = "方舟多账号调度中心：编排 MAA 与 mower 的多账号自动化",
    after_help = "文档：docs/akops-design.md（设计）· akops init 开始"
)]
struct Cli {
    /// 工作目录（默认：cwd 向上逐级探测 akops.toml）
    #[arg(long, global = true, value_name = "DIR")]
    workdir: Option<PathBuf>,

    /// maa-cli 安装根（覆盖 akops.toml paths.maa_dir）
    #[arg(long, global = true, value_name = "DIR")]
    maa_dir: Option<PathBuf>,

    /// mower 检出目录（覆盖 akops.toml paths.mower_dir）
    #[arg(long, global = true, value_name = "DIR")]
    mower_dir: Option<PathBuf>,

    /// 替代配置文件路径（默认 <workdir>/akops.toml）
    #[arg(long, global = true, value_name = "FILE")]
    config: Option<PathBuf>,

    /// 日志详细度（-v=debug，-vv=trace；或 RUST_LOG）
    #[arg(short = 'v', long, global = true, action = ArgAction::Count)]
    verbose: u8,

    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// 初始化工作目录（目录骨架 + 环境探测写入 akops.toml）
    Init {
        /// 目标目录（默认当前目录）
        #[arg(long, value_name = "DIR")]
        dir: Option<PathBuf>,
    },
    /// 环境体检：adb/设备/maa/mower/docker/账号唯一性/端口段
    Doctor,
    /// 启动 daemon + Web 控制台【M1 任务 8】
    Server {
        /// 监听端口（覆盖配置）
        #[arg(long)]
        port: Option<u16>,
        /// 监听地址（覆盖配置）
        #[arg(long)]
        bind: Option<String>,
        /// 启动后自动打开浏览器
        #[arg(long)]
        open: bool,
    },
    /// 总览：账号/设备/活跃会话/队列/退避
    Status,
    /// 账号管理
    Account {
        #[command(subcommand)]
        cmd: commands::account::AccountCmd,
    },
    /// 设备管理
    Device {
        #[command(subcommand)]
        cmd: commands::device::DeviceCmd,
    },
    /// 账号预置引导（人工登录一次）【M1 任务 5】
    Provision {
        /// 账号 id
        account: String,
        /// 设备名（缺省=唯一注册设备）
        #[arg(long)]
        device: Option<String>,
    },
    /// 手动切号（MAA 官方能力，短暂持有设备租约）【M1 任务 4】
    Switch {
        /// 账号 id
        account: String,
        /// 设备名（缺省=唯一注册设备）
        #[arg(long)]
        device: Option<String>,
        /// 超时（如 3m）
        #[arg(long)]
        timeout: Option<String>,
    },
    /// 会话管理【M1 任务 6/7】
    Session {
        #[command(subcommand)]
        cmd: commands::todo_placeholder::SessionCmd,
    },
    /// 调度器开关【M1 任务 7】
    Schedule {
        #[command(subcommand)]
        cmd: commands::todo_placeholder::ScheduleCmd,
    },
    /// maa-cli 包装：install/update/version【M1 任务 4】
    Maa {
        #[command(subcommand)]
        cmd: commands::todo_placeholder::MaaCmd,
    },
    /// mower 更新（pin commit，含回滚）【M1 任务 6】
    Mower {
        #[command(subcommand)]
        cmd: commands::todo_placeholder::MowerCmd,
    },
    /// 导出工作目录为 zip（可脱敏）【M1 任务 9】
    Export {
        /// 输出文件（默认 export/akops-export-YYYYMMDD.zip）
        #[arg(long)]
        out: Option<PathBuf>,
        /// 按字段名清单脱敏密钥
        #[arg(long)]
        redact_secrets: bool,
        /// 仅导出统计
        #[arg(long)]
        stats_only: bool,
    },
    /// 从 zip 导入工作目录【M1 任务 9】
    Import {
        /// zip 文件
        file: PathBuf,
        /// 与现有内容合并
        #[arg(long, group = "mode")]
        merge: bool,
        /// 替换现有内容
        #[arg(long, group = "mode")]
        replace: bool,
    },
    /// 手动执行 SQLite schema 迁移
    Migrate,
    /// 生成 shell 补全脚本
    Completions {
        /// 目标 shell
        #[arg(value_enum)]
        shell: Shell,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    init_tracing(cli.verbose);

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("构建 tokio 运行时失败");

    let result: anyhow::Result<()> = rt.block_on(async move { dispatch(cli).await });
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("错误：{e:#}");
            ExitCode::FAILURE
        }
    }
}

async fn dispatch(cli: Cli) -> anyhow::Result<()> {
    use Cmd::*;
    use commands::todo_placeholder as todo;

    // 字段级访问：command 被 move，其余字段仍可借用
    let wd = || require_workdir(cli.workdir.as_deref(), cli.config.as_deref());
    match cli.command {
        Init { dir } => commands::init::run(dir.as_deref()).await,
        Doctor => commands::doctor::run(wd()?, cli.mower_dir.as_deref()).await,
        Status => commands::status::run(wd()?).await,
        Account { cmd } => commands::account::run(wd()?, cmd).await,
        Device { cmd } => commands::device::run(wd()?, cmd).await,
        Completions { shell } => {
            generate(shell, &mut Cli::command(), "akops", &mut std::io::stdout());
            Ok(())
        }
        Server { .. } => todo::server(),
        Provision { account, device } => todo::provision(&account, device.as_deref()),
        Switch {
            account,
            device,
            timeout,
        } => todo::switch(&account, device.as_deref(), timeout.as_deref()),
        Session { cmd } => todo::session(cmd),
        Schedule { cmd } => todo::schedule(cmd),
        Maa { cmd } => todo::maa(cmd),
        Mower { cmd } => todo::mower(cmd),
        Export { .. } => todo::export(),
        Import { .. } => todo::import(),
        Migrate => todo::migrate(),
    }
}

/// 解析全局参数得到工作目录：--config 替代配置文件（其父目录为 workdir）；
/// --workdir 显式指定；否则从 cwd 向上探测 akops.toml（§15）。
fn require_workdir(workdir: Option<&Path>, config: Option<&Path>) -> anyhow::Result<Workdir> {
    if let Some(cfg) = config {
        let dir = cfg
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        return Ok(Workdir::new(dir));
    }
    match workdir {
        Some(d) => Ok(Workdir::new(d)),
        None => {
            let cwd = std::env::current_dir()?;
            Workdir::discover(&cwd).map_err(anyhow::Error::from)
        }
    }
}

fn init_tracing(verbosity: u8) {
    use tracing_subscriber::EnvFilter;
    let default_level = match verbosity {
        0 => "info",
        1 => "debug",
        _ => "trace",
    };
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_level));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_target(false)
        .init();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_parses_rails_style() {
        // 核心子命令族可解析（设计文档 §15 命令表）
        let ok_cases: &[&[&str]] = &[
            &["akops", "init", "--dir", "/tmp/x"],
            &["akops", "doctor"],
            &["akops", "status"],
            &["akops", "--workdir", "/tmp/w", "doctor"],
            &[
                "akops",
                "device",
                "add",
                "d1",
                "--host-adb",
                "127.0.0.1:2771",
            ],
            &[
                "akops",
                "account",
                "add",
                "main",
                "--server",
                "official",
                "--account-name",
                "1***2",
            ],
            &["akops", "switch", "main", "--device", "d1"],
            &["akops", "session", "start", "main", "--slice", "90m"],
            &["akops", "schedule", "pause"],
            &["akops", "maa", "update", "--resource-only"],
            &["akops", "mower", "update", "--ref", "abc123"],
            &["akops", "export", "--redact-secrets"],
            &["akops", "import", "x.zip", "--replace"],
            &["akops", "completions", "bash"],
        ];
        for case in ok_cases {
            let r = Cli::try_parse_from(case.iter().copied());
            assert!(r.is_ok(), "{case:?} 解析失败：{r:?}");
        }
        let r = Cli::try_parse_from(["akops", "account", "add", "main", "--server", "bogus"]);
        assert!(r.is_err(), "非法 server 应被拒绝");
    }
}
