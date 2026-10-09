//! 工作目录单写者锁（ADR-0001 D7）：`state/.workdir.lock`。
//!
//! 规则（设计文档 §4.3）：daemon 持有此锁常驻；无 daemon 时写类 CLI 命令
//! 必须先排他获取；获取失败 = daemon 在运行 = CLI 不得直接写（须走 API）。
//! 只读命令永不取锁。

use std::path::Path;
use std::time::Duration;

use fslock::LockFile;

use crate::error::{CoreError, Result};

/// 持有的工作目录写锁；Drop 自动释放。
pub struct WorkdirGuard {
    _file: LockFile,
    path: std::path::PathBuf,
}

impl WorkdirGuard {
    fn open(path: &Path) -> Result<LockFile> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| CoreError::io(path, e))?;
        }
        LockFile::open(path)
            .map_err(|e| CoreError::Other(format!("打开锁文件 {} 失败：{e}", path.display())))
    }

    /// 非阻塞尝试获取；`None` = 已被 daemon 持有。
    pub fn try_acquire(path: &Path) -> Result<Option<WorkdirGuard>> {
        let mut file = Self::open(path)?;
        if file
            .try_lock()
            .map_err(|e| CoreError::Other(format!("加锁 {} 失败：{e}", path.display())))?
        {
            Ok(Some(WorkdirGuard {
                _file: file,
                path: path.to_path_buf(),
            }))
        } else {
            Ok(None)
        }
    }

    /// 阻塞获取（带总超时）。
    pub fn acquire(path: &Path, timeout: Duration) -> Result<WorkdirGuard> {
        let mut file = Self::open(path)?;
        let deadline = std::time::Instant::now() + timeout;
        loop {
            match file.try_lock() {
                Ok(true) => {
                    return Ok(WorkdirGuard {
                        _file: file,
                        path: path.to_path_buf(),
                    });
                }
                Ok(false) => {
                    if std::time::Instant::now() >= deadline {
                        return Err(CoreError::Other(format!(
                            "等待工作目录锁超时（{}）：daemon 正在运行，写操作请走其 API",
                            path.display()
                        )));
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                Err(e) => {
                    return Err(CoreError::Other(format!(
                        "加锁 {} 失败：{e}",
                        path.display()
                    )));
                }
            }
        }
    }

    /// 锁文件路径。
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// 写类命令入口的统一约定：取锁失败时给出面向用户的指引。
pub fn describe_lock_conflict(wd: &crate::config::Workdir) -> String {
    let daemon = wd.daemon_json();
    if daemon.exists() {
        format!(
            "daemon 似乎正在运行（{}）：写操作请走 Web API；确认已停止 daemon 后再试",
            daemon.display()
        )
    } else {
        "工作目录被其他 arkreunion 进程占用".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exclusive_and_reentrant_after_drop() {
        let tmp = tempfile::tempdir().unwrap();
        let lock = tmp.path().join(".workdir.lock");
        let g1 = WorkdirGuard::try_acquire(&lock)
            .unwrap()
            .expect("首次应成功");
        assert_eq!(g1.path(), lock);
        // 第二把拿不到
        assert!(WorkdirGuard::try_acquire(&lock).unwrap().is_none());
        // 有限超时也拿不到
        assert!(WorkdirGuard::acquire(&lock, Duration::from_millis(150)).is_err());
        drop(g1);
        // 释放后可再取
        let g2 = WorkdirGuard::try_acquire(&lock).unwrap();
        assert!(g2.is_some());
    }

    #[test]
    fn blocking_acquire_succeeds_after_release() {
        let tmp = tempfile::tempdir().unwrap();
        let lock = tmp.path().join(".workdir.lock");
        let g = WorkdirGuard::try_acquire(&lock).unwrap().unwrap();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            drop(g);
        });
        let g2 = WorkdirGuard::acquire(&lock, Duration::from_secs(5)).unwrap();
        assert_eq!(g2.path(), lock);
    }
}
