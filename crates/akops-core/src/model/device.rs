//! 设备模型（设计文档 §6.2）。

use crate::error::Result;
use crate::model::account::validate_slug;
use serde::{Deserialize, Serialize};

/// 设备后端类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DeviceBackendKind {
    /// 已存在、只连不管（redroid 容器/MuMu/雷电/物理机……），跨平台
    External,
    /// akops 全生命周期管理（bollard 建池/扩容），仅 Linux（M2）
    Redroid,
}

impl std::str::FromStr for DeviceBackendKind {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "external" => Ok(DeviceBackendKind::External),
            "redroid" => Ok(DeviceBackendKind::Redroid),
            other => Err(format!("backend {other:?} 不合法：external|redroid")),
        }
    }
}

impl std::fmt::Display for DeviceBackendKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            DeviceBackendKind::External => "external",
            DeviceBackendKind::Redroid => "redroid",
        })
    }
}

/// 设备连接配置——**双 adb 地址**（设计文档 §6.2 审查完善点）：
/// 同一设备对宿主进程与容器内进程呈现不同地址，物化器按 Runner 形态选择。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceConnection {
    /// 宿主视角地址（如 `127.0.0.1:2771`）——ProcessRunner / 宿主侧 maa-cli 使用
    pub host_adb: String,
    /// 容器网络视角地址（如 `arknights:5555`）——DockerRunner 使用
    pub docker_adb: Option<String>,
    /// DockerRunner 需加入的网络名
    pub docker_network: Option<String>,
}

/// 设备（`devices/<name>.toml`）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Device {
    /// 唯一标识（= 文件名去后缀，slug）
    pub name: String,
    /// 后端类型
    pub backend: DeviceBackendKind,
    /// 连接配置
    pub connection: DeviceConnection,
    /// 备注
    #[serde(default)]
    pub notes: String,
}

impl Device {
    /// 校验设备配置。
    pub fn validate(&self) -> Result<()> {
        validate_slug(&self.name, "设备名")?;
        if self.connection.host_adb.is_empty() {
            return Err(crate::error::CoreError::Config(format!(
                "设备 {} 缺少 connection.host_adb",
                self.name
            )));
        }
        Ok(())
    }

    /// 双地址齐备才算「Docker 兼容」（DockerRunner 可用）。
    pub fn docker_compatible(&self) -> bool {
        self.connection.docker_adb.is_some() && self.connection.docker_network.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_toml_roundtrip() {
        let src = r#"
name = "redroid-main"
backend = "external"

[connection]
host_adb = "127.0.0.1:2771"
docker_adb = "arknights:5555"
docker_network = "arknights_default"

notes = "现有单实例，nvidia GPU"
"#;
        let dev: Device = toml::from_str(src).unwrap();
        assert_eq!(dev.backend, DeviceBackendKind::External);
        assert!(dev.docker_compatible());
        dev.validate().unwrap();

        let out = toml::to_string_pretty(&dev).unwrap();
        assert_eq!(toml::from_str::<Device>(&out).unwrap(), dev);
    }

    #[test]
    fn minimal_device_notes_default() {
        let dev: Device = toml::from_str(
            r#"
name = "mumu"
backend = "external"
[connection]
host_adb = "127.0.0.1:16384"
"#,
        )
        .unwrap();
        assert_eq!(dev.notes, "");
        assert!(!dev.docker_compatible());
    }
}
