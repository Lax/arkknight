//! `arkknight init`：初始化工作目录（§15）。
//!
//! 目录骨架 + 环境探测（adb/maa/mower/docker）写入 arkknight.toml；
//! 拒绝覆盖已初始化目录。

use std::path::Path;

use anyhow::{Context, bail};

use arkknight_core::config::{AkopsConfig, Workdir, detect};
use arkknight_core::model::{Device, DeviceBackendKind, DeviceConnection};

pub(crate) async fn run(dir: Option<&Path>) -> anyhow::Result<()> {
    let target: &Path = dir.unwrap_or_else(|| Path::new("."));
    let target = if target.is_absolute() {
        target.to_path_buf()
    } else {
        std::env::current_dir()
            .context("解析当前目录失败")?
            .join(target)
    };

    println!("初始化工作目录 {} …", target.display());
    let det = detect::detect(&detect::DetectOverrides {
        adb_path: None,
        mower_dir: None,
    })
    .await;

    let mut cfg = AkopsConfig::default();

    // paths：探测结果写入配置（保持可移植默认值优先）
    match &det.adb {
        Some(adb) => {
            // PATH 中的 adb 保留相对名；显式路径用绝对路径
            cfg.paths.adb_path = if adb
                .path
                .parent()
                .map(|p| p.iter().count() > 1)
                .unwrap_or(false)
                && adb.path.is_absolute()
                && !target.join("adb").exists()
            {
                // 绝对路径探测到 → 写绝对路径，避免 PATH 差异
                adb.path.to_string_lossy().into_owned()
            } else {
                "adb".into()
            };
            println!("  ✓ adb        {}（{}）", adb.path.display(), adb.version);
        }
        None => println!("  ✗ adb        未找到（请安装 platform-tools 后重跑 init）"),
    }
    match &det.maa {
        Some(m) => println!(
            "  ✓ maa-cli    {}（{}，MaaCore {}）",
            m.cli.path.display(),
            m.cli.version,
            m.core_version.as_deref().unwrap_or("未安装")
        ),
        None => println!("  ✗ maa-cli    未找到（切号依赖，见 docs/arkknight-design.md §3.2）"),
    }
    match &det.mower {
        Some(m) => {
            cfg.paths.mower_dir = m.path.to_string_lossy().into_owned();
            println!(
                "  ✓ mower      {}（{}）",
                m.path.display(),
                match (&m.branch, &m.commit) {
                    (Some(b), Some(c)) => format!("{b}@{c}"),
                    _ => "非 git 检出".into(),
                }
            );
        }
        None => println!("  - mower      未找到（可用 --mower-dir 指定；DockerRunner 可后补）"),
    }
    match &det.python {
        Some(p) => println!("  ✓ python     {}", p.version),
        None => println!("  - python     未找到（ProcessRunner 需要 3.11+）"),
    }
    match &det.docker {
        Some(d) => println!("  ✓ docker     daemon {}", d.version),
        None => println!("  - docker     不可达（DockerRunner 需要；Windows 可用 ProcessRunner）"),
    }

    let wd = Workdir::init(&target, &cfg).context("初始化工作目录失败")?;
    println!("  ✓ 配置       {}", wd.config_path().display());

    // 便捷起步：若探测到唯一候选 mower 检出以外，还发现本机常见 adb 端口，则提示（不自动注册设备）
    println!();
    println!("下一步：");
    println!("  1. arkknight device add <name> --host-adb 127.0.0.1:<port>   # 注册模拟器");
    println!("  2. arkknight account add <id> --server official --account-name '123****8901'");
    println!("  3. arkknight doctor                                          # 体检");
    println!(
        "  4. arkknight provision <id> --device <name>                  # 人工登录一次（切号前提）"
    );
    Ok(())
}

/// device add 共用的构造与落盘逻辑。
pub(crate) fn build_and_save_device(
    wd: &Workdir,
    name: &str,
    host_adb: String,
    docker_adb: Option<String>,
    docker_network: Option<String>,
    backend: DeviceBackendKind,
    notes: String,
) -> anyhow::Result<()> {
    if backend == DeviceBackendKind::Redroid {
        bail!("redroid 设备后端属于 M2（设计文档 §20）；M1 请注册 external 设备");
    }
    let dev = Device {
        name: name.to_string(),
        backend,
        connection: DeviceConnection {
            host_adb,
            docker_adb,
            docker_network,
        },
        notes,
    };
    dev.validate().map_err(anyhow::Error::from)?;
    if wd.device_file(&dev.name).exists() {
        bail!(
            "设备 {} 已存在（如需修改请直接编辑 {}）",
            dev.name,
            wd.device_file(&dev.name).display()
        );
    }
    wd.save_device(&dev).map_err(anyhow::Error::from)?;
    Ok(())
}
