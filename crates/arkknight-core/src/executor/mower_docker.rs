//! MowerExecutor 的 DockerRunner 形态（§8.3，ADR-0001 D5）。
//!
//! bollard 起停会话容器：镜像内带 mower 代码（`cmd = python3 run_server.py`，
//! 工作目录 `/mower`），bundle 与 tmp 以 bind mount 直读直写——用户经深链
//! mower UI 的改动天然持久化（§11.4），adb 用设备 `docker_adb` 视角。

use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use bollard::models::{ContainerCreateBody, HostConfig, PortBinding};
use bollard::query_parameters::{
    CreateContainerOptions, InspectContainerOptions, RemoveContainerOptions, StopContainerOptions,
};
use bollard::{API_DEFAULT_VERSION, Docker};

use crate::executor::{Executor, ExecutorError, ExecutorHandle, ExecutorHealth, SessionCtx};
use crate::model::{ExecutorKind, RunnerKind};

/// 镜像内约定路径（与 arknights-mower 镜像 Dockerfile 一致，见 docs/dev/docker-runner.md）。
pub const MOWER_ROOT_IN_CONTAINER: &str = "/mower";
const CONNECT_TIMEOUT_SECS: u64 = 30;

/// 解析 `paths.docker_host` 形态的端点并建立客户端。
/// 空/None → 本机默认（Linux unix socket、Windows 命名管道）；容器内部署
/// 指向 socket-proxy（`tcp://socket-proxy:2375`）。
pub fn connect_docker(host: Option<&str>) -> Result<Docker, ExecutorError> {
    let h = host.map(str::trim).filter(|s| !s.is_empty());
    let docker = match h {
        None => Docker::connect_with_local_defaults(),
        Some(s) if s.starts_with("unix://") || s.starts_with("npipe://") => {
            Docker::connect_with_socket(s, CONNECT_TIMEOUT_SECS, API_DEFAULT_VERSION)
        }
        // tcp:// 与 http(s):// 都交由 connect_with_http（内部剥 tcp:// 前缀）
        Some(s) => Docker::connect_with_http(s, CONNECT_TIMEOUT_SECS, API_DEFAULT_VERSION),
    }
    .map_err(|e| ExecutorError::Start(format!("连接 Docker（{h:?}）失败：{e}")))?;
    Ok(docker)
}

/// 纯函数构造容器 spec：单测覆盖挂载/端口/网络，无需 Docker daemon。
/// `container_port` = 会话分配的 webview 端口（容器内外同号，1:1 发布到 127.0.0.1）。
pub fn container_spec(
    image: &str,
    bundle_dir: &Path,
    tmp_dir: &Path,
    port: u16,
    token: &str,
    network: Option<&str>,
) -> ContainerCreateBody {
    let root = MOWER_ROOT_IN_CONTAINER;
    let exposed = format!("{port}/tcp");
    let mut port_bindings = HashMap::new();
    port_bindings.insert(
        exposed.clone(),
        Some(vec![PortBinding {
            host_ip: Some("127.0.0.1".into()),
            host_port: Some(port.to_string()),
        }]),
    );
    ContainerCreateBody {
        image: Some(image.to_string()),
        working_dir: Some(root.to_string()),
        // 与镜像入口一致：mower 会话即 run_server.py（§8.3）
        cmd: Some(vec!["python3".into(), "run_server.py".into()]),
        env: Some(vec![
            format!("MOWER_DATA_DIR={root}"),
            format!("ARKKNIGHT_SESSION_TOKEN={token}"),
        ]),
        exposed_ports: Some(vec![exposed.clone()]),
        host_config: Some(HostConfig {
            binds: Some(vec![
                format!("{}:{root}/config", bundle_dir.display()),
                format!("{}:{root}/tmp", tmp_dir.display()),
            ]),
            port_bindings: Some(port_bindings),
            network_mode: network.map(str::to_string),
            ..Default::default()
        }),
        ..Default::default()
    }
}

pub struct MowerDockerExecutor {
    docker: Docker,
    image: String,
}

/// 停止并删除会话容器（drain 与 CLI `session stop` 共用）。
pub async fn stop_and_remove(
    docker: &Docker,
    container: &str,
    grace: Duration,
) -> Result<(), ExecutorError> {
    let t = i32::try_from(grace.as_secs().min(i64::from(i32::MAX) as u64)).unwrap_or(i32::MAX);
    if let Err(e) = docker
        .stop_container(
            container,
            Some(StopContainerOptions {
                signal: None,
                t: Some(t),
            }),
        )
        .await
    {
        tracing::warn!("停止容器 {container} 失败（将强删）：{e}");
    }
    docker
        .remove_container(
            container,
            Some(RemoveContainerOptions {
                force: true,
                ..Default::default()
            }),
        )
        .await
        .map_err(|e| ExecutorError::Drain(format!("删除容器 {container} 失败：{e}")))
}

impl MowerDockerExecutor {
    pub fn new(docker: Docker, image: impl Into<String>) -> Self {
        MowerDockerExecutor {
            docker,
            image: image.into(),
        }
    }

    fn container_name(session_id: u64) -> String {
        format!("arkknight-mower-s{session_id}")
    }
}

#[async_trait::async_trait]
impl Executor for MowerDockerExecutor {
    fn kind(&self) -> ExecutorKind {
        ExecutorKind::Mower
    }

    async fn start(&self, ctx: &SessionCtx) -> Result<ExecutorHandle, ExecutorError> {
        let port = ctx.mower_port.ok_or_else(|| {
            ExecutorError::Start("mower 会话须分配 webview 端口（SessionCtx.mower_port）".into())
        })?;
        let docker_adb = ctx.endpoints.docker_adb.clone().ok_or_else(|| {
            ExecutorError::Start(
                "设备未配置 docker_adb：DockerRunner 的 adb 需容器网络视角地址（§11.3）"
                    .to_string(),
            )
        })?;
        let network = ctx.endpoints.docker_network.as_deref();
        let account_dir = ctx.workdir.join("accounts").join(&ctx.account.key);
        let bundle = account_dir.join("mower");
        if !bundle.join("conf.yml").is_file() {
            return Err(ExecutorError::Start(format!(
                "账号 bundle 缺少 {}/conf.yml（从现有部署导入或手工放置）",
                bundle.display()
            )));
        }
        let tmp = account_dir.join("mower-data").join("tmp");
        std::fs::create_dir_all(&tmp)
            .map_err(|e| ExecutorError::Start(format!("建 {} 失败：{e}", tmp.display())))?;

        // 会话启动瞬间：白名单改写 conf.yml（§11.4 时序约束）；adb 用 docker 视角
        let conf = bundle.join("conf.yml");
        let raw = std::fs::read_to_string(&conf)
            .map_err(|e| ExecutorError::Start(format!("读取 {} 失败：{e}", conf.display())))?;
        let patch = crate::materialize::MowerPatch {
            adb: docker_adb,
            port,
            token: Some(ctx.webview_token()),
            start_automatically: true,
        };
        let patched = crate::materialize::patch_conf(&raw, &patch)
            .map_err(|e| ExecutorError::Start(e.to_string()))?;
        std::fs::write(&conf, patched)
            .map_err(|e| ExecutorError::Start(format!("写回 {} 失败：{e}", conf.display())))?;

        let name = Self::container_name(ctx.session_id);
        // 幂等：同号残留容器先清理（daemon 崩溃后的重跑场景）
        let _ = self
            .docker
            .remove_container(
                &name,
                Some(RemoveContainerOptions {
                    force: true,
                    ..Default::default()
                }),
            )
            .await;

        let spec = container_spec(
            &self.image,
            &bundle,
            &tmp,
            port,
            &ctx.webview_token(),
            network,
        );
        self.docker
            .create_container(
                Some(CreateContainerOptions {
                    name: Some(name.clone()),
                    platform: String::new(),
                }),
                spec,
            )
            .await
            .map_err(|e| {
                ExecutorError::Start(format!(
                    "创建容器 {name} 失败：{e}（镜像 {} 不存在？docker pull 或先构建，见 docs/dev/docker-runner.md）",
                    self.image
                ))
            })?;
        self.docker
            .start_container(&name, None)
            .await
            .map_err(|e| ExecutorError::Start(format!("启动容器 {name} 失败：{e}")))?;

        Ok(ExecutorHandle {
            session_id: ctx.session_id,
            kind: self.kind(),
            runner: RunnerKind::Docker,
            locator: format!("container={name} port={port}"),
        })
    }

    async fn drain(&self, handle: &ExecutorHandle, grace: Duration) -> Result<(), ExecutorError> {
        let name = handle
            .locator
            .split_whitespace()
            .find_map(|p| p.strip_prefix("container="))
            .ok_or_else(|| {
                ExecutorError::Drain(format!("locator 无 container=：{}", handle.locator))
            })?
            .to_string();
        stop_and_remove(&self.docker, &name, grace).await
    }

    async fn health(&self, handle: &ExecutorHandle) -> ExecutorHealth {
        let Some(name) = handle
            .locator
            .split_whitespace()
            .find_map(|p| p.strip_prefix("container="))
        else {
            return ExecutorHealth::Dead;
        };
        match self
            .docker
            .inspect_container(name, None::<InspectContainerOptions>)
            .await
        {
            Ok(info) => match info.state.and_then(|s| s.running) {
                Some(true) => ExecutorHealth::Alive,
                Some(false) => ExecutorHealth::Dead,
                None => ExecutorHealth::Unknown,
            },
            Err(_) => ExecutorHealth::Dead,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn container_spec_mounts_ports_network() {
        let spec = container_spec(
            "arkknight-mower:latest",
            Path::new("/wd/accounts/a1/mower"),
            Path::new("/wd/accounts/a1/mower-data/tmp"),
            58123,
            "arkknight-s9",
            Some("arknights_default"),
        );
        assert_eq!(spec.image.as_deref(), Some("arkknight-mower:latest"));
        assert_eq!(
            spec.cmd.as_deref(),
            Some(&["python3".to_string(), "run_server.py".to_string()][..])
        );
        let hc = spec.host_config.unwrap();
        let binds = hc.binds.unwrap();
        assert!(
            binds
                .iter()
                .any(|b| b == "/wd/accounts/a1/mower:/mower/config"),
            "{binds:?}"
        );
        assert!(
            binds
                .iter()
                .any(|b| b == "/wd/accounts/a1/mower-data/tmp:/mower/tmp")
        );
        assert_eq!(hc.network_mode.as_deref(), Some("arknights_default"));
        let pb = hc.port_bindings.unwrap();
        let b = pb.get("58123/tcp").unwrap().as_ref().unwrap();
        assert_eq!(b[0].host_ip.as_deref(), Some("127.0.0.1"));
        assert_eq!(b[0].host_port.as_deref(), Some("58123"));
        assert!(
            spec.env
                .as_ref()
                .unwrap()
                .iter()
                .any(|e| e == "MOWER_DATA_DIR=/mower")
        );
    }

    #[test]
    fn connect_rejects_missing_socket() {
        let r = connect_docker(Some("unix:///nonexistent/arkknight-test.sock"));
        assert!(r.is_err());
    }
}
