//! 核心库统一错误类型。

use std::path::PathBuf;

/// akops-core 顶层错误。设备/执行器子层有各自的错误类型
/// （[`crate::device::DeviceError`] / [`crate::executor::ExecutorError`]），
/// 由调度器层（M1 任务 7）负责归并。
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("配置错误：{0}")]
    Config(String),

    #[error("找不到 {0}")]
    NotFound(String),

    #[error("IO 错误（{path}）：{source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("{0}")]
    Other(String),
}

impl CoreError {
    /// 以路径构造 IO 错误，统一携带出错文件，便于排障。
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        CoreError::Io {
            path: path.into(),
            source,
        }
    }
}

/// 核心库 Result 别名。
pub type Result<T, E = CoreError> = std::result::Result<T, E>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn io_error_carries_path() {
        let err = CoreError::io(
            "/tmp/x.toml",
            std::io::Error::new(std::io::ErrorKind::NotFound, "no"),
        );
        let msg = err.to_string();
        assert!(msg.contains("/tmp/x.toml"));
        assert!(msg.contains("no"));
    }
}
