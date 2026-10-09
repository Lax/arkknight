//! 设备层：DeviceBackend trait（§7.1）+ external 后端（§7.2）。
//!
//! INV-3：调度器/物化器只面向 [`DeviceBackend`] trait 与双地址端点编程；
//! redroid Docker 后端（建池/水位准入）为 M2 交付，trait 预留。

pub mod external;

use serde::Serialize;

use crate::error::CoreError;

pub use external::ExternalBackend;

/// 设备错误。
#[derive(Debug, thiserror::Error)]
pub enum DeviceError {
    /// 该后端不支持此操作（如 External 的 provision/stop）
    #[error("设备后端 {0} 不支持该操作")]
    Unsupported(&'static str),

    #[error("设备不可达：{0}")]
    Unreachable(String),

    #[error("adb 操作失败：{0}")]
    Adb(String),

    #[error("{0}")]
    Other(String),
}

/// 双视角端点（物化器按 Runner 形态消费，§6.2 双地址设计）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeviceEndpoints {
    /// 宿主视角（ProcessRunner / 宿主 maa-cli）
    pub host_adb: String,
    /// 容器网络视角（DockerRunner）
    pub docker_adb: Option<String>,
    pub docker_network: Option<String>,
}

/// 设备健康状态。
#[derive(Debug, Clone, Serialize)]
pub struct DeviceHealth {
    pub reachable: bool,
    /// adb 状态/错误细节（`device`/`offline`/`unauthorized`/…）
    pub detail: String,
}

/// 宿主水位（M2 扩容准入；§7.3）。
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Watermark {
    pub free_mem_gb: f64,
    pub cpu_idle_pct: f64,
    pub free_gpu_mem_gb: Option<f64>,
}

/// redroid 实例规格（M2 模板实例化入参）。
#[derive(Debug, Clone)]
pub struct RedroidSpec {
    pub image: String,
    pub host_port: u16,
    pub mem_limit_gb: u32,
    pub cpu_limit: u32,
    pub gpu: bool,
}

/// 设备后端（§7.1 M1 版签名，允许 M2 前修订）。
///
/// M1 只要求 `name/health/endpoints`；生命周期与水位方法带默认实现返回
/// [`DeviceError::Unsupported`]，由 M2 的 RedroidDocker 覆盖。
#[async_trait::async_trait]
pub trait DeviceBackend: Send + Sync {
    /// 设备名（对应 `devices/<name>.toml`）。
    fn name(&self) -> &str;

    /// 设备是否在线可达（adb connect + 状态校验）。
    async fn health(&self) -> Result<DeviceHealth, DeviceError>;

    /// 已安装的明日方舟相关包名列表（`device test` 语义，§7.2）。
    /// 后端无关的实现走 adb；不支持的后端返回空。
    async fn detect_game_packages(&self) -> Result<Vec<String>, DeviceError> {
        Ok(Vec::new())
    }

    /// 宿主/容器双视角地址。
    fn endpoints(&self) -> DeviceEndpoints;

    /// 启动一个新实例（M2：RedroidDocker 模板实例化），返回设备名。
    async fn provision(&self, _spec: RedroidSpec) -> Result<String, DeviceError> {
        Err(DeviceError::Unsupported("provision"))
    }

    async fn start(&self) -> Result<(), DeviceError> {
        Err(DeviceError::Unsupported("start"))
    }

    async fn stop(&self) -> Result<(), DeviceError> {
        Err(DeviceError::Unsupported("stop"))
    }

    async fn destroy(&self) -> Result<(), DeviceError> {
        Err(DeviceError::Unsupported("destroy"))
    }

    /// 宿主水位（M2 扩容准入）。
    async fn host_watermark(&self) -> Result<Watermark, DeviceError> {
        Err(DeviceError::Unsupported("host_watermark"))
    }
}

/// 构造设备后端实例（M1 仅 external）。
pub fn build_backend(
    dev: &crate::model::Device,
    adb_path: &std::path::Path,
) -> Result<Box<dyn DeviceBackend>, CoreError> {
    match dev.backend {
        crate::model::DeviceBackendKind::External => {
            Ok(Box::new(ExternalBackend::new(dev.clone(), adb_path)))
        }
        crate::model::DeviceBackendKind::Redroid => Err(CoreError::Config(
            "redroid 设备后端属于 M2（设计文档 §20），M1 请注册 external 设备".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn external_lifecycle_ops_unsupported() {
        let dev = crate::model::Device {
            name: "d".into(),
            backend: crate::model::DeviceBackendKind::External,
            connection: crate::model::DeviceConnection {
                host_adb: "127.0.0.1:1".into(),
                docker_adb: None,
                docker_network: None,
            },
            notes: String::new(),
        };
        let b = ExternalBackend::new(dev, std::path::Path::new("adb"));
        assert!(matches!(
            b.provision(RedroidSpec {
                image: "x".into(),
                host_port: 28000,
                mem_limit_gb: 4,
                cpu_limit: 2,
                gpu: false,
            })
            .await,
            Err(DeviceError::Unsupported("provision"))
        ));
        assert!(matches!(
            b.host_watermark().await,
            Err(DeviceError::Unsupported(_))
        ));
    }
}
