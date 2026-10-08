//! 环境探测：adb / maa-cli / mower 检出 / python / docker（`akops init` 与 `doctor` 共用）。
//!
//! 全部通过外部进程探测（与 §16 一致：platform-tools/模拟器属外部依赖，doctor 探测）。

use std::env;
use std::path::{Path, PathBuf};

use serde::Serialize;

/// 在 PATH 中查找可执行文件。
pub fn which(name: &str) -> Option<PathBuf> {
    let exe_name = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    };
    let path = env::var_os("PATH")?;
    env::split_paths(&path)
        .map(|dir| dir.join(&exe_name))
        .find(|p| p.is_file())
}

async fn run_capture(program: &str, args: &[&str], cwd: Option<&Path>) -> Option<String> {
    let mut cmd = tokio::process::Command::new(program);
    cmd.args(args).stdin(std::process::Stdio::null());
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    let out = tokio::time::timeout(std::time::Duration::from_secs(15), cmd.output())
        .await
        .ok()?
        .ok()?;
    if out.status.success() {
        Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        None
    }
}

/// 探测到的外部工具信息。
#[derive(Debug, Clone, Serialize)]
pub struct ToolInfo {
    pub path: PathBuf,
    pub version: String,
}

/// maa-cli + MaaCore。
#[derive(Debug, Clone, Serialize)]
pub struct MaaInfo {
    pub cli: ToolInfo,
    /// MaaCore 版本（`maa version` 第二行；缺失时 None）
    pub core_version: Option<String>,
}

/// mower 检出信息。
#[derive(Debug, Clone, Serialize)]
pub struct MowerInfo {
    pub path: PathBuf,
    /// 分支名（detached HEAD 时为 "HEAD"）；非 git 检出为 None
    pub branch: Option<String>,
    /// 短 commit
    pub commit: Option<String>,
}

/// Docker daemon。
#[derive(Debug, Clone, Serialize)]
pub struct DockerInfo {
    pub version: String,
}

/// 环境探测结果。
#[derive(Debug, Clone, Default, Serialize)]
pub struct Detection {
    pub adb: Option<ToolInfo>,
    pub maa: Option<MaaInfo>,
    pub mower: Option<MowerInfo>,
    pub python: Option<ToolInfo>,
    pub docker: Option<DockerInfo>,
}

/// 探测时的路径覆盖（来自 CLI 全局参数 / akops.toml）。
#[derive(Debug, Clone, Default)]
pub struct DetectOverrides {
    /// 显式 adb 路径（配置 `paths.adb_path`），None=查 PATH 的 `adb`
    pub adb_path: Option<String>,
    /// 显式 mower 检出（配置 `paths.mower_dir`）
    pub mower_dir: Option<String>,
}

/// 探测 adb：二进制 + 版本首行。
pub async fn detect_adb(explicit: Option<&str>) -> Option<ToolInfo> {
    let path = match explicit {
        Some(p) if !p.is_empty() => {
            let pb = PathBuf::from(p);
            if !pb.is_file() {
                return None;
            }
            pb
        }
        _ => which("adb")?,
    };
    let version = run_capture(&path.to_string_lossy(), &["version"], None)
        .await
        .and_then(|out| out.lines().next().map(String::from))?;
    Some(ToolInfo { path, version })
}

/// 探测 maa-cli 与 MaaCore：`maa version` 输出形如
/// `maa-cli v0.7.5` / `MaaCore v6.17.5`。
pub async fn detect_maa() -> Option<MaaInfo> {
    let path = which("maa")?;
    let out = run_capture(&path.to_string_lossy(), &["version"], None).await?;
    let mut cli_version = None;
    let mut core_version = None;
    for line in out.lines() {
        if let Some(v) = line.trim().strip_prefix("maa-cli") {
            cli_version = Some(v.trim().to_string());
        } else if let Some(v) = line.trim().strip_prefix("MaaCore") {
            core_version = Some(v.trim().to_string());
        }
    }
    Some(MaaInfo {
        cli: ToolInfo {
            path,
            version: cli_version.unwrap_or_else(|| "未知版本".into()),
        },
        core_version,
    })
}

/// mower 检出的常规候选路径（无显式配置时依次尝试）。
pub fn mower_dir_candidates() -> Vec<PathBuf> {
    let mut cands: Vec<PathBuf> = Vec::new();
    if let Some(v) = env::var_os("ARKOPS_MOWER_DIR") {
        cands.push(PathBuf::from(v));
    }
    cands.push(PathBuf::from("arknights-mower"));
    if let Some(home) = dirs::home_dir() {
        cands.push(home.join("src").join("arknights-mower"));
        cands.push(home.join("arknights-mower"));
    }
    cands
}

/// 探测 mower 检出（首个存在的候选或显式路径；读取 git 信息）。
pub async fn detect_mower(explicit: Option<&str>) -> Option<MowerInfo> {
    let path = match explicit {
        Some(p) if !p.is_empty() => {
            let pb = crate::config::expand_path(p);
            if !pb.is_dir() {
                return None;
            }
            pb
        }
        _ => mower_dir_candidates().into_iter().find(|p| p.is_dir())?,
    };
    let branch = run_capture("git", &["rev-parse", "--abbrev-ref", "HEAD"], Some(&path)).await;
    let commit = run_capture("git", &["rev-parse", "--short", "HEAD"], Some(&path)).await;
    Some(MowerInfo {
        path,
        branch,
        commit,
    })
}

/// 探测 python3（Windows 回退 `python`；ProcessRunner 需 3.11+，§16）。
pub async fn detect_python() -> Option<ToolInfo> {
    for name in if cfg!(windows) {
        ["python", "python3"]
    } else {
        ["python3", "python"]
    } {
        let path = which(name)?;
        if let Some(out) = run_capture(&path.to_string_lossy(), &["--version"], None).await {
            return Some(ToolInfo {
                path,
                version: out.trim().to_string(),
            });
        }
    }
    None
}

/// 探测 Docker daemon 可达性。
pub async fn detect_docker() -> Option<DockerInfo> {
    which("docker")?;
    let version = run_capture("docker", &["info", "--format", "{{.ServerVersion}}"], None).await?;
    Some(DockerInfo { version })
}

/// 全量探测。
pub async fn detect(ov: &DetectOverrides) -> Detection {
    let (adb, maa, mower, python, docker) = tokio::join!(
        detect_adb(ov.adb_path.as_deref()),
        detect_maa(),
        detect_mower(ov.mower_dir.as_deref()),
        detect_python(),
        detect_docker(),
    );
    Detection {
        adb,
        maa,
        mower,
        python,
        docker,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn which_finds_shell_builtin_like_tools_or_none() {
        // 不假设环境有特定工具：只验证接口行为
        let _ = which("adb");
        assert!(which("definitely-not-a-real-tool-akops-xyz").is_none());
    }

    #[tokio::test]
    async fn detect_is_total() {
        // 全量探测在任何机器上都不应 panic；缺失项为 None
        let d = detect(&DetectOverrides::default()).await;
        if let Some(maa) = &d.maa {
            assert!(maa.cli.version.contains('v') || maa.cli.version == "未知版本");
        }
        if let Some(py) = &d.python {
            assert!(py.version.to_lowercase().contains("python"));
        }
    }
}
