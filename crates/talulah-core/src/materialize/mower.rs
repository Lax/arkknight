//! mower conf.yml 白名单改写（§11.3/§11.4 审查完善点）。
//!
//! 会话启动瞬间仅改写 4 个字段：`adb`、`start_automatically`、
//! `webview.port`、`webview.token`；**其余键原样保留**——用户在 mower UI
//! 的其他编辑是事实源的一部分，绝不重写整文件语义。
//!
//! 注：serde_yaml 序列化不保留注释与键序；白名单改写发生在会话启动瞬间且
//! 只影响这 4 个键的语义。若未来需要注释保真，换 patch 式编辑器并保持
//! 本模块 API 不变。

use crate::error::{CoreError, Result};
use crate::materialize::MowerPatch;
use serde_yaml::{Mapping, Value};

/// 对 `conf.yml` 文本执行白名单改写，返回新文本。
pub fn patch_conf(input: &str, patch: &MowerPatch) -> Result<String> {
    let mut root: Value = serde_yaml::from_str(input)
        .map_err(|e| CoreError::Config(format!("conf.yml 不是合法 YAML：{e}")))?;
    if !root.is_mapping() {
        return Err(CoreError::Config(
            "conf.yml 顶层必须是映射（mapping）".into(),
        ));
    }
    let map = root.as_mapping_mut().expect("已校验为 mapping");

    map.insert(Value::from("adb"), Value::from(patch.adb.as_str()));
    map.insert(
        Value::from("start_automatically"),
        Value::from(patch.start_automatically),
    );

    // webview 段存在则改，不存在则建（其余键保留）
    let webview = map
        .entry(Value::from("webview"))
        .or_insert_with(|| Value::Mapping(Mapping::new()));
    if !webview.is_mapping() {
        return Err(CoreError::Config(
            "conf.yml 的 webview 段不是映射，拒绝改写（请检查配置）".into(),
        ));
    }
    let wv = webview.as_mapping_mut().expect("已校验为 mapping");
    wv.insert(Value::from("port"), Value::from(patch.port));
    if let Some(tok) = &patch.token {
        wv.insert(Value::from("token"), Value::from(tok.as_str()));
    }

    serde_yaml::to_string(&root)
        .map_err(|e| CoreError::Config(format!("序列化 conf.yml 失败：{e}")))
}

/// 确保 ProcessRunner 的数据目录（§8.3）：`mower-data/config -> ../mower` 符号链接 + `tmp/`。
///
/// 返回数据目录路径。Windows 无符号链接权限时给出明确错误（跨平台限制，§16）。
pub fn ensure_process_data_dir(account_dir: &std::path::Path) -> Result<std::path::PathBuf> {
    let data = account_dir.join("mower-data");
    let config_link = data.join("config");
    let tmp = data.join("tmp");
    std::fs::create_dir_all(&tmp).map_err(|e| CoreError::io(&tmp, e))?;
    if !config_link.exists() && !config_link.is_symlink() {
        symlink_dir(&account_dir.join("mower"), &config_link)?;
    }
    Ok(data)
}

/// 跨平台目录符号链接。
fn symlink_dir(target: &std::path::Path, link: &std::path::Path) -> Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link).map_err(|e| CoreError::io(link, e))
    }
    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_dir(target, link).map_err(|e| {
            CoreError::Other(format!(
                "创建符号链接 {} 失败：{e}（Windows 需开发者模式或管理员权限；\
                 也可改用 DockerRunner）",
                link.display()
            ))
        })
    }
}

/// 生成 ProcessRunner 启动器（`run_server.py`，仿现有部署的包装器：
/// 按 conf 读取 token/port 起 Flask）。akops 拥有此文件（会话启动时重写，幂等）。
pub fn render_process_launcher(mower_checkout: &std::path::Path) -> String {
    let checkout = mower_checkout.to_string_lossy().replace('\\', "\\\\");
    format!(
        r#"# 由 akops 生成（ProcessRunner 会话启动器）；请勿手改
import sys
sys.path.insert(0, r"{checkout}")

from server import app
from arknights_mower.utils import config

conf = config.conf
token = conf.webview.token
host = "0.0.0.0" if token else "127.0.0.1"
port = conf.webview.port if token else 5000

if token:
    app.token = token

print(f"mower 会话启动：http://127.0.0.1:{{port}}/?token={{token}}", flush=True)
app.run(host=host, port=port)
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn patch(port: u16) -> MowerPatch {
        MowerPatch {
            adb: "arknights:5555".into(),
            port,
            token: Some("sess-token".into()),
            start_automatically: true,
        }
    }

    const SAMPLE: &str = "\
# 基建配置（用户可编辑）
plan_times: &pt
  '23:50': [Recreation, 0]
skland_info:
  - account: user@example.com
    password: '***'
webview:
  port: 59000
  token: old-token
adb: 127.0.0.1:9999
";

    #[test]
    fn patches_whitelist_and_keeps_rest() {
        let out = patch_conf(SAMPLE, &patch(58100)).unwrap();
        assert!(out.contains("adb: arknights:5555"), "{out}");
        assert!(out.contains("start_automatically: true"));
        assert!(out.contains("port: 58100"));
        assert!(out.contains("token: sess-token"));
        // 用户数据原样保留
        assert!(out.contains("user@example.com"));
        assert!(out.contains("Recreation"));
        assert!(!out.contains("127.0.0.1:9999"));
        assert!(!out.contains("old-token"));
    }

    #[test]
    fn creates_missing_webview_section() {
        let input = "adb: x\n";
        let out = patch_conf(input, &patch(58101)).unwrap();
        assert!(out.contains("port: 58101"), "{out}");
        assert!(out.contains("token: sess-token"));
        assert!(out.contains("adb: arknights:5555"));
    }

    #[test]
    fn token_none_preserves_existing() {
        let p = MowerPatch {
            token: None,
            ..patch(58102)
        };
        let out = patch_conf(SAMPLE, &p).unwrap();
        assert!(out.contains("token: old-token"), "{out}");
    }

    #[test]
    fn idempotent() {
        let once = patch_conf(SAMPLE, &patch(58100)).unwrap();
        let twice = patch_conf(&once, &patch(58100)).unwrap();
        assert_eq!(once, twice);
    }

    #[test]
    fn rejects_non_mapping_and_bad_yaml() {
        assert!(patch_conf("- a\n- b\n", &patch(1)).is_err());
        assert!(patch_conf("a: [unclosed", &patch(1)).is_err());
    }
}
