//! 物化器 golden-file 测试（AGENTS.md 开发约定：物化器必须配 golden 快照）。
//!
//! 快照位于 `tests/golden/`；设 `AKOPS_UPDATE_GOLDEN=1` 重新生成。
//! 平台相关内容（MAA connection.config）在文件名中以 `.posix`/`.win32` 区分。

use std::path::PathBuf;
use std::process::exit;

use akops_core::device::DeviceEndpoints;
use akops_core::materialize::{MowerPatch, render_profile, render_startup_task};
use akops_core::model::{Account, RunnerKind, Server};

fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

fn update_mode() -> bool {
    std::env::var("AKOPS_UPDATE_GOLDEN").is_ok()
}

fn platform_suffix() -> &'static str {
    if cfg!(windows) { "win32" } else { "posix" }
}

fn assert_golden(name: &str, actual: &str) {
    let path = golden_dir().join(name);
    if update_mode() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, actual).unwrap();
        println!("已更新快照 {}", path.display());
        return;
    }
    match std::fs::read_to_string(&path) {
        Ok(expected) => {
            assert!(
                expected == actual,
                "快照不一致：{}\n--- expected ---\n{expected}\n--- actual ---\n{actual}",
                path.display()
            );
        }
        Err(_) => {
            eprintln!(
                "缺少快照 {}：请以 AKOPS_UPDATE_GOLDEN=1 cargo test 生成",
                path.display()
            );
            exit(1);
        }
    }
}

fn account() -> Account {
    Account {
        id: "main".into(),
        display_name: "主号".into(),
        server: Server::Official,
        account_name: "123****8901".into(),
        uid: None,
        enabled: true,
        schedule: Default::default(),
        provisioned_on: vec![],
    }
}

fn endpoints() -> DeviceEndpoints {
    DeviceEndpoints {
        host_adb: "127.0.0.1:2771".into(),
        docker_adb: Some("arknights:5555".into()),
        docker_network: Some("arknights_default".into()),
    }
}

#[test]
fn golden_profile_process() {
    let out = render_profile(&account(), &endpoints(), RunnerKind::Process).unwrap();
    assert_golden(&format!("profile_process.{}.toml", platform_suffix()), &out);
}

#[test]
fn golden_profile_docker() {
    let out = render_profile(&account(), &endpoints(), RunnerKind::Docker).unwrap();
    assert_golden(&format!("profile_docker.{}.toml", platform_suffix()), &out);
}

#[test]
fn golden_startup_task() {
    let out = render_startup_task(&account()).unwrap();
    assert_golden("startup_task.toml", &out);
}

#[test]
fn golden_mower_patch() {
    let input = std::fs::read_to_string(golden_dir().join("mower_conf_in.yaml")).unwrap();
    let patch = MowerPatch {
        adb: "arknights:5555".into(),
        port: 58100,
        token: Some("session-token-abc".into()),
        start_automatically: true,
    };
    let out = akops_core::materialize::patch_conf(&input, &patch).unwrap();
    assert_golden("mower_conf_out.yaml", &out);
}
