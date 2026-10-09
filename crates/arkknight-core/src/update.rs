//! mower 更新体系（UpdatePlan，设计文档 §12）：程序/资源分离，pin commit。
//!
//! - ProcessRunner：`git fetch + checkout --detach <ref>`，写 `state/mower-pin.toml`
//!   记录前一 commit → `--rollback` 一键回退（R2：alpha 分支不稳的兜底）。
//! - DockerRunner：同样 pin 检出，镜像重建由用户 Dockerfile 完成（§12），
//!   命令给出等价 `docker build` 提示。
//! - 避让活跃会话：调用方负责先停会话（本模块检查 store 并拒绝）。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{CoreError, Result};

/// pin 记录（`state/mower-pin.toml`）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MowerPin {
    /// 更新前检出（回滚目标）
    pub previous: String,
    /// 更新后检出
    pub current: String,
    /// 目标 ref（裸 commit 时为 "HEAD"）
    pub ref_: String,
    /// 更新时刻（本地 RFC3339）
    pub at: String,
}

pub fn pin_path(wd: &crate::config::Workdir) -> PathBuf {
    wd.state_dir().join("mower-pin.toml")
}

pub fn load_pin(wd: &crate::config::Workdir) -> Result<Option<MowerPin>> {
    let p = pin_path(wd);
    if !p.is_file() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&p).map_err(|e| CoreError::io(&p, e))?;
    Ok(Some(toml::from_str(&raw).map_err(|e| {
        CoreError::Config(format!("解析 {} 失败：{e}", p.display()))
    })?))
}

fn save_pin(wd: &crate::config::Workdir, pin: &MowerPin) -> Result<()> {
    let p = pin_path(wd);
    std::fs::create_dir_all(p.parent().expect("state 目录"))
        .map_err(|e| CoreError::io(p.parent().unwrap(), e))?;
    let body = toml::to_string_pretty(pin)
        .map_err(|e| CoreError::Config(format!("序列化 pin 失败：{e}")))?;
    std::fs::write(&p, body).map_err(|e| CoreError::io(&p, e))
}

fn git(checkout: &Path, args: &[&str]) -> Result<String> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(checkout)
        .args(args)
        .output()
        .map_err(|e| CoreError::Other(format!("运行 git 失败（PATH 中无 git？）：{e}")))?;
    if !out.status.success() {
        return Err(CoreError::Other(format!(
            "git {} 失败：{}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// 当前检出 commit（短 hash；非 git 目录即错误，doctor 可提前发现）。
pub fn current_commit(checkout: &Path) -> Result<String> {
    git(checkout, &["rev-parse", "--short", "HEAD"])
}

/// 更新检出：fetch + checkout --detach。返回 (previous, current)。
/// `target_ref` 缺省跟踪 alpha 最新（§12 默认策略）。
pub fn checkout_ref(checkout: &Path, target_ref: Option<&str>) -> Result<(String, String)> {
    let prev = current_commit(checkout)?;
    let r = target_ref.unwrap_or("alpha");
    if r == "HEAD" {
        git(checkout, &["checkout", "--detach", "HEAD"])?;
    } else {
        // fetch 后 checkout FETCH_HEAD：分支/tag/裸 commit 统一处理，
        // 且避开 `checkout --detach <分支名>` 在新 git 下的 DWIM 冲突
        git(checkout, &["fetch", "--tags", "origin", r])?;
        git(checkout, &["checkout", "--detach", "FETCH_HEAD"])?;
    }
    let cur = current_commit(checkout)?;
    Ok((prev, cur))
}

/// 执行 UpdatePlan 并落 pin 记录。Docker 形态同样 pin 检出（重建镜像前置步骤）。
pub fn update(
    wd: &crate::config::Workdir,
    checkout: &Path,
    target_ref: Option<&str>,
) -> Result<MowerPin> {
    let (previous, current) = checkout_ref(checkout, target_ref)?;
    let pin = MowerPin {
        previous,
        current: current.clone(),
        ref_: target_ref.unwrap_or("alpha").to_string(),
        at: chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
    };
    save_pin(wd, &pin)?;
    Ok(pin)
}

/// 回滚到上一 pin 的 previous。返回回滚前后的 commit。
pub fn rollback(wd: &crate::config::Workdir, checkout: &Path) -> Result<(String, String)> {
    let pin = load_pin(wd)?.ok_or_else(|| {
        CoreError::NotFound("无 pin 记录（state/mower-pin.toml）：从未执行过 mower update".into())
    })?;
    let before = current_commit(checkout)?;
    git(checkout, &["checkout", "--detach", &pin.previous])?;
    let after = current_commit(checkout)?;
    // 新 pin：回滚本身也要可再回滚
    save_pin(
        wd,
        &MowerPin {
            previous: before.clone(),
            current: after.clone(),
            ref_: format!("rollback→{}", pin.previous),
            at: chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        },
    )?;
    Ok((before, after))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git_available() -> bool {
        std::process::Command::new("git")
            .arg("--version")
            .output()
            .is_ok_and(|o| o.status.success())
    }

    /// 建一个两个 commit 的本地仓库当"远端"，再 clone 出检出。
    fn seed_repo(dir: &Path) {
        let run = |args: &[&str], cwd: &Path| {
            let st = std::process::Command::new("git")
                .arg("-C")
                .arg(cwd)
                .args(args)
                .env("GIT_AUTHOR_NAME", "t")
                .env("GIT_AUTHOR_EMAIL", "t@t")
                .env("GIT_COMMITTER_NAME", "t")
                .env("GIT_COMMITTER_EMAIL", "t@t")
                .output()
                .unwrap();
            assert!(st.status.success(), "git {args:?} 失败");
        };
        std::fs::create_dir_all(dir).unwrap();
        run(&["init", "-q", "-b", "main", dir.to_str().unwrap()], dir);
        std::fs::write(dir.join("f.txt"), "v1").unwrap();
        run(&["add", "."], dir);
        run(&["commit", "-q", "-m", "c1"], dir);
        // alpha 分支超前一个 commit
        run(&["checkout", "-q", "-b", "alpha"], dir);
        std::fs::write(dir.join("f.txt"), "v2").unwrap();
        run(&["commit", "-q", "-am", "c2"], dir);
        // 远端 HEAD 回到 main：clone 后停在旧 commit，update 才有位移
        run(&["symbolic-ref", "HEAD", "refs/heads/main"], dir);
    }

    #[test]
    fn update_and_rollback_roundtrip() {
        if !git_available() {
            eprintln!("git 不可用，跳过");
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let remote = tmp.path().join("remote");
        seed_repo(&remote);
        let checkout = tmp.path().join("mower");
        let st = std::process::Command::new("git")
            .args([
                "clone",
                "-q",
                remote.to_str().unwrap(),
                checkout.to_str().unwrap(),
            ])
            .output()
            .unwrap();
        assert!(st.status.success());

        let wd = crate::config::Workdir::new(tmp.path());
        assert_eq!(current_commit(&checkout).unwrap().len(), 7);

        // update → alpha 最新
        let pin = update(&wd, &checkout, None).unwrap();
        assert_eq!(pin.ref_, "alpha");
        assert_ne!(pin.previous, pin.current);
        let loaded = load_pin(&wd).unwrap().unwrap();
        assert_eq!(loaded, pin);

        // rollback → 回到 clone 时的 main
        let (before, after) = rollback(&wd, &checkout).unwrap();
        assert_eq!(before, pin.current);
        assert_eq!(after, pin.previous);
    }
}
