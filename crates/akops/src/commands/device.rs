//! `akops device`：设备管理（§15；test = health + 游戏包检测，§7.2）。

use anyhow::Result;
use clap::Subcommand;

use akops_core::config::Workdir;
use akops_core::device::build_backend;
use akops_core::model::{Device, DeviceBackendKind};

use crate::commands::init::build_and_save_device;

#[derive(Subcommand, Debug)]
pub enum DeviceCmd {
    /// 注册设备（external：已存在、只连不管）
    Add {
        /// 设备名（slug）
        name: String,
        /// 宿主视角 adb 地址（ProcessRunner / 宿主 maa-cli 用）
        #[arg(long)]
        host_adb: String,
        /// 容器网络 adb 地址（DockerRunner 用）
        #[arg(long)]
        docker_adb: Option<String>,
        /// DockerRunner 加入的网络名
        #[arg(long)]
        docker_network: Option<String>,
        /// 后端类型（M1 仅 external）
        #[arg(long, default_value = "external")]
        backend: DeviceBackendKind,
        /// 备注
        #[arg(long, default_value = "")]
        notes: String,
    },
    /// 列出设备
    List,
    /// 查看设备配置
    Show { name: String },
    /// 删除设备注册
    Remove {
        name: String,
        #[arg(long)]
        yes: bool,
    },
    /// 连通性测试：adb connect + 状态 + 游戏包检测
    Test { name: String },
}

pub(crate) async fn run(wd: Workdir, cmd: DeviceCmd) -> Result<()> {
    match cmd {
        DeviceCmd::Add {
            name,
            host_adb,
            docker_adb,
            docker_network,
            backend,
            notes,
        } => {
            build_and_save_device(
                &wd,
                &name,
                host_adb,
                docker_adb,
                docker_network,
                backend,
                notes,
            )?;
            println!("✓ 设备 {name} 已注册：{}", wd.device_file(&name).display());
            println!("  下一步：akops device test {name}");
            Ok(())
        }
        DeviceCmd::List => {
            for d in wd.load_all_devices().map_err(anyhow::Error::from)? {
                let docker = if d.docker_compatible() {
                    " docker✓"
                } else {
                    ""
                };
                println!(
                    "{}\t{}\thost={}{}{}",
                    d.name,
                    d.backend,
                    d.connection.host_adb,
                    docker,
                    {
                        if d.notes.is_empty() {
                            String::new()
                        } else {
                            format!("\t# {}", d.notes)
                        }
                    }
                );
            }
            Ok(())
        }
        DeviceCmd::Show { name } => {
            let _ = wd.load_device(&name).map_err(anyhow::Error::from)?;
            print!(
                "{}",
                std::fs::read_to_string(wd.device_file(&name)).map_err(|e| anyhow::anyhow!(e))?
            );
            Ok(())
        }
        DeviceCmd::Remove { name, yes } => {
            if !yes {
                anyhow::bail!("确认删除设备 {name} 请加 --yes（仅删注册，不动模拟器本体）");
            }
            wd.remove_device(&name).map_err(anyhow::Error::from)?;
            println!("✓ 设备 {name} 已删除");
            Ok(())
        }
        DeviceCmd::Test { name } => {
            let cfg = wd.load_config().map_err(anyhow::Error::from)?;
            let dev: Device = wd.load_device(&name).map_err(anyhow::Error::from)?;
            let backend =
                build_backend(&dev, &cfg.paths.adb_path_expanded()).map_err(anyhow::Error::from)?;
            match backend.health().await {
                Ok(h) if h.reachable => {
                    println!(
                        "设备 {name}（{}）：在线（adb device）",
                        dev.connection.host_adb
                    );
                    match backend.detect_game_packages().await {
                        Ok(pkgs) if !pkgs.is_empty() => {
                            for p in &pkgs {
                                println!("  游戏包: {p}");
                            }
                        }
                        Ok(_) => println!("  ⚠ 未检测到明日方舟游戏包"),
                        Err(e) => println!("  ⚠ 游戏包检测失败：{e}"),
                    }
                    Ok(())
                }
                Ok(h) => anyhow::bail!("设备 {name} 不在线（adb 状态：{}）", h.detail),
                Err(e) => Err(anyhow::anyhow!("设备 {name} 探测失败：{e}")),
            }
        }
    }
}
