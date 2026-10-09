//! External 设备后端（§7.2）：已存在、只连不管，跨平台。
//!
//! 只依赖 adb：`adb connect <host_adb>` → 状态校验；不做生命周期管理。
//! Windows 下 `host_adb` 指向 MuMu/雷电等模拟器的 adb 端口即可用。

use std::path::{Path, PathBuf};

use crate::device::{DeviceBackend, DeviceEndpoints, DeviceError, DeviceHealth};
use crate::model::Device;

/// External 后端实例。
pub struct ExternalBackend {
    device: Device,
    adb_path: PathBuf,
}

impl ExternalBackend {
    pub fn new(device: Device, adb_path: &Path) -> Self {
        ExternalBackend {
            device,
            adb_path: adb_path.to_path_buf(),
        }
    }

    /// 已安装的明日方舟相关包名列表（官服/B 服，§7.2）。
    async fn detect_game_packages_impl(&self) -> Result<Vec<String>, DeviceError> {
        let out = self
            .adb(&[
                "shell",
                "pm",
                "list",
                "packages",
                "com.hypergryph.arknights",
            ])
            .await?;
        let pkgs: Vec<String> = out
            .lines()
            .filter_map(|l| l.trim().strip_prefix("package:").map(String::from))
            .collect();
        Ok(pkgs)
    }

    async fn adb(&self, args: &[&str]) -> Result<String, DeviceError> {
        let serial = self.device.connection.host_adb.as_str();
        let mut cmd = tokio::process::Command::new(&self.adb_path);
        cmd.args(["-s", serial])
            .args(args)
            .stdin(std::process::Stdio::null());
        let out = tokio::time::timeout(std::time::Duration::from_secs(20), cmd.output())
            .await
            .map_err(|_| DeviceError::Adb(format!("{serial} 执行 {args:?} 超时")))?
            .map_err(|e| DeviceError::Adb(format!("spawn adb 失败：{e}")))?;
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        if !out.status.success() {
            return Err(DeviceError::Adb(format!(
                "adb {args:?} 退出码 {:?}：{}",
                out.status.code(),
                combined.trim()
            )));
        }
        Ok(combined.trim().to_string())
    }
}

#[async_trait::async_trait]
impl DeviceBackend for ExternalBackend {
    fn name(&self) -> &str {
        &self.device.name
    }

    async fn detect_game_packages(&self) -> Result<Vec<String>, DeviceError> {
        self.detect_game_packages_impl().await
    }

    fn endpoints(&self) -> DeviceEndpoints {
        DeviceEndpoints {
            host_adb: self.device.connection.host_adb.clone(),
            docker_adb: self.device.connection.docker_adb.clone(),
            docker_network: self.device.connection.docker_network.clone(),
        }
    }

    async fn health(&self) -> Result<DeviceHealth, DeviceError> {
        let serial = self.device.connection.host_adb.as_str();
        // 1) connect（对网络 adb 幂等；USB 设备会报错，忽略后用 devices 校验）
        let _ = {
            let mut cmd = tokio::process::Command::new(&self.adb_path);
            cmd.args(["connect", serial])
                .stdin(std::process::Stdio::null());
            tokio::time::timeout(std::time::Duration::from_secs(10), cmd.output())
                .await
                .ok()
                .and_then(|r| r.ok())
        };
        // 2) get-state 校验
        let out = self.adb(&["get-state"]).await.map_err(|e| match e {
            DeviceError::Adb(msg)
                if msg.contains("not found")
                    || msg.contains("offline")
                    || msg.contains("unauthorized") =>
            {
                DeviceError::Unreachable(msg)
            }
            other => other,
        })?;
        let state = out.trim().to_string();
        Ok(DeviceHealth {
            reachable: state == "device",
            detail: state,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn backend() -> ExternalBackend {
        ExternalBackend::new(
            Device {
                name: "t".into(),
                backend: crate::model::DeviceBackendKind::External,
                connection: crate::model::DeviceConnection {
                    host_adb: "127.0.0.1:1".into(), // 不存在的端口
                    docker_adb: None,
                    docker_network: None,
                },
                notes: String::new(),
            },
            Path::new(if cfg!(windows) { "adb.exe" } else { "adb" }),
        )
    }

    #[tokio::test]
    async fn unreachable_device_reports_unreachable_or_adb_error() {
        // 无真机环境时：要么 adb 不存在（spawn 失败），要么连接失败；都不应 panic
        match backend().health().await {
            Err(DeviceError::Unreachable(_)) | Err(DeviceError::Adb(_)) => {}
            Ok(h) => panic!("死端口不应可达：{h:?}"),
            Err(e) => panic!("意外错误：{e}"),
        }
    }

    #[test]
    fn endpoints_mirror_connection() {
        let b = backend();
        let ep = b.endpoints();
        assert_eq!(ep.host_adb, "127.0.0.1:1");
        assert!(ep.docker_adb.is_none());
    }
}
