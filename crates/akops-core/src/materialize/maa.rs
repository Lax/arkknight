//! MAA 配置物化（§11.3 + 附录 B）：
//! - `profiles/default.toml`：连接配置（切号前运行期改写 address）
//! - `tasks/startup.toml`：StartUp 任务（account_name 参数 = INV-1 官方切号）

use crate::config::adb_address_for;
use crate::device::DeviceEndpoints;
use crate::error::{CoreError, Result};
use crate::model::{Account, RunnerKind};
use serde::Serialize;

/// maa-cli profile 的连接段。
#[derive(Debug, Serialize)]
struct ConnectionSection {
    address: String,
    config: &'static str,
}

/// maa-cli profile 的实例选项段。
#[derive(Debug, Serialize)]
struct InstanceOptions {
    /// redroid 场景 ADB 触控最稳（设计文档附录 B）
    touch_mode: &'static str,
}

/// maa-cli profile（`profiles/default.toml`）。
#[derive(Debug, Serialize)]
struct MaaProfile {
    connection: ConnectionSection,
    instance_options: InstanceOptions,
}

/// StartUp 任务参数（MAA 官方开始唤醒）。
#[derive(Debug, Serialize)]
struct StartUpParams {
    client_type: &'static str,
    /// 切号匹配串；空串时 MAA 仅启动游戏不选号
    account_name: String,
    start_game_enabled: bool,
}

/// StartUp 任务条目。
#[derive(Debug, Serialize)]
struct StartUpTask {
    name: &'static str,
    #[serde(rename = "type")]
    task_type: &'static str,
    params: StartUpParams,
}

/// StartUp 任务文件（`[[tasks]]` 形态，附录 B）。
#[derive(Debug, Serialize)]
struct TaskFile {
    tasks: Vec<StartUpTask>,
}

/// 生成 MAA profile TOML 文本。
///
/// `runner` 决定写入哪个 adb 地址（双地址规则 §8.3）：DockerRunner 用
/// `docker_adb`，否则 `host_adb`。
pub fn render_profile(
    account: &Account,
    endpoints: &DeviceEndpoints,
    runner: RunnerKind,
) -> Result<String> {
    let dev = device_like(account, endpoints)?;
    let address = adb_address_for(&dev, runner)?;
    let profile = MaaProfile {
        connection: ConnectionSection {
            address,
            config: crate::materialize::maa_connection_config(),
        },
        instance_options: InstanceOptions { touch_mode: "ADB" },
    };
    toml::to_string_pretty(&profile)
        .map_err(|e| CoreError::Config(format!("序列化 MAA profile 失败：{e}")))
}

/// 生成 StartUp 任务 TOML（`tasks/startup.toml` 唯一内容；INV-1 官方切号）。
pub fn render_startup_task(account: &Account) -> Result<String> {
    let file = TaskFile {
        tasks: vec![StartUpTask {
            name: "切号并启动",
            task_type: "StartUp",
            params: StartUpParams {
                client_type: account.server.maa_client_type(),
                account_name: account.account_name.clone(),
                start_game_enabled: true,
            },
        }],
    };
    let mut out = String::from(
        "# 由 akops 物化器生成（INV-1：官方开始唤醒切号）；请勿手改，自定义任务请另建文件\n",
    );
    out += &toml::to_string_pretty(&file)
        .map_err(|e| CoreError::Config(format!("序列化 MAA task 失败：{e}")))?;
    Ok(out)
}

/// 把「账号 + 端点」临时拼成 [`Device`] 以复用双地址选择规则。
fn device_like(_account: &Account, endpoints: &DeviceEndpoints) -> Result<crate::model::Device> {
    use crate::model::{Device, DeviceBackendKind, DeviceConnection};
    Ok(Device {
        name: String::new(),
        backend: DeviceBackendKind::External,
        connection: DeviceConnection {
            host_adb: endpoints.host_adb.clone(),
            docker_adb: endpoints.docker_adb.clone(),
            docker_network: endpoints.docker_network.clone(),
        },
        notes: String::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Server;

    fn account(server: Server, name: &str) -> Account {
        Account {
            id: "main".into(),
            display_name: "主号".into(),
            server,
            account_name: name.into(),
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
    fn profile_selects_address_by_runner() {
        let acc = account(Server::Official, "123****8901");
        let ep = endpoints();
        let host = render_profile(&acc, &ep, RunnerKind::Process).unwrap();
        assert!(host.contains(r#"address = "127.0.0.1:2771""#), "{host}");
        assert!(host.contains("CompatPOSIXShell") || host.contains("CompatWin32Shell"));
        assert!(host.contains(r#"touch_mode = "ADB""#));

        let docker = render_profile(&acc, &ep, RunnerKind::Docker).unwrap();
        assert!(docker.contains(r#"address = "arknights:5555""#), "{docker}");

        // 幂等
        assert_eq!(
            host,
            render_profile(&acc, &ep, RunnerKind::Process).unwrap()
        );
    }

    #[test]
    fn profile_rejects_missing_docker_address() {
        let acc = account(Server::Official, "1***2");
        let ep = DeviceEndpoints {
            host_adb: "127.0.0.1:16384".into(),
            docker_adb: None,
            docker_network: None,
        };
        assert!(render_profile(&acc, &ep, RunnerKind::Docker).is_err());
    }

    #[test]
    fn startup_task_per_server() {
        let official = render_startup_task(&account(Server::Official, "123****8901")).unwrap();
        assert!(
            official.contains(r#"client_type = "Official""#),
            "{official}"
        );
        assert!(official.contains(r#"account_name = "123****8901""#));
        assert!(official.contains(r#"type = "StartUp""#));
        assert!(official.contains("start_game_enabled = true"));

        let bili = render_startup_task(&account(Server::Bilibili, "阿米娅")).unwrap();
        assert!(bili.contains(r#"client_type = "Bilibili""#), "{bili}");
        assert!(bili.contains(r#"account_name = "阿米娅""#));

        // 幂等
        assert_eq!(
            official,
            render_startup_task(&account(Server::Official, "123****8901")).unwrap()
        );
    }
}
