//! 子命令实现（设计文档 §15）。

pub mod account;
pub mod device;
pub mod doctor;
pub mod init;
pub mod maa;
pub mod provision;
pub mod schedule;
pub mod server;
pub mod session;
pub mod status;
pub mod switch_;
pub mod todo_placeholder;

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};

use arkreunion_core::config::Workdir;
use arkreunion_core::lock::{WorkdirGuard, describe_lock_conflict};
use arkreunion_core::store::Store;

/// 写类命令的统一入口约定（ADR-0001 D7）：daemon 存活（锁被持有）则拒绝直写。
pub(crate) fn write_lock(wd: &Workdir) -> Result<WorkdirGuard> {
    match WorkdirGuard::try_acquire(&wd.lock_file())? {
        Some(g) => Ok(g),
        None => anyhow::bail!("{}", describe_lock_conflict(wd)),
    }
}

/// 打开（或创建）运行态数据库。
pub(crate) fn open_store(wd: &Workdir) -> Result<Arc<Store>> {
    let store = Store::open(&wd.db_path()).context("打开运行态数据库失败")?;
    Ok(Arc::new(store))
}

/// 入口处保守回收孤儿租约（CLI 场景 daemon 不在；崩溃残留的租约 1h 后可回收）。
pub(crate) fn recover_orphans(store: &Store) {
    match store.recover_orphan_leases(Duration::from_secs(3600)) {
        Ok(devices) if !devices.is_empty() => {
            tracing::warn!("回收孤儿租约：{}", devices.join(", "));
        }
        Ok(_) => {}
        Err(e) => tracing::warn!("孤儿租约回收失败：{e}"),
    }
}

/// 解析 --device：缺省时要求恰好注册了一台设备。
pub(crate) fn resolve_device(
    wd: &Workdir,
    device: Option<&str>,
) -> Result<arkreunion_core::model::Device> {
    match device {
        Some(name) => wd.load_device(name).map_err(anyhow::Error::from),
        None => {
            let names = wd.device_names();
            match names.as_slice() {
                [only] => wd.load_device(only).map_err(anyhow::Error::from),
                [] => anyhow::bail!("工作目录未注册任何设备：先 arkreunion device add"),
                many => anyhow::bail!("注册了多台设备，请用 --device 指定：{}", many.join(", ")),
            }
        }
    }
}
