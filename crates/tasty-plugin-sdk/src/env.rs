//! Plugin이 호스트로부터 받는 환경변수를 안전하게 읽는 헬퍼.
//!
//! 호스트는 spawn 시 다음 환경변수를 주입한다 (`docs/agent-guide/plugins.md` 참조):
//! - `TASTY_PLUGIN_ID`
//! - `TASTY_PLUGIN_DIR`
//! - `TASTY_PLUGIN_DATA_DIR`
//! - `TASTY_PLUGIN_CONFIG_PATH`
//! - `TASTY_PLUGIN_LOG_PATH`
//! - `TASTY_HOST_IPC_PORT`
//! - `TASTY_PLUGIN_TOKEN`
//! - `TASTY_HOST_API_VERSION`
//! - `TASTY_LOCALE` (선택) — 활성 언어 코드 (예: `en`, `ko`, `ja`). 미주입 시 `en` fallback.
//! - `TASTY_PLUGIN_HANDLE_ENDPOINT` (선택) — Unix는 socket path, Windows는 Named Pipe 이름.
//!   호스트가 보조 핸들 채널을 활성화한 경우에만 주입된다. plugin은 이 값이 있으면
//!   메인 채널 인증 후 보조 채널에도 connect를 시도한다.

use std::path::PathBuf;

use crate::error::{PluginError, Result};

#[derive(Debug, Clone)]
pub struct PluginEnv {
    pub plugin_id: String,
    pub host_port: u16,
    pub token: String,
    pub host_api_version: String,
    pub plugin_dir: Option<PathBuf>,
    pub data_dir: Option<PathBuf>,
    pub config_path: Option<PathBuf>,
    pub log_path: Option<PathBuf>,
    /// 활성 언어 코드 (예: `en`, `ko`, `ja`). 호스트가 `TASTY_LOCALE`로 주입.
    /// 미주입 시 `"en"`. plugin이 자체 lang 파일을 로드할 때 사용.
    pub locale: String,
    /// 보조 핸들 채널 endpoint. 호스트가 활성화한 경우에만 `Some`.
    /// Unix는 socket path (`/tmp/.../tasty-handle.sock`), Windows는 Named Pipe 이름
    /// (`\\.\pipe\tasty-handle-<random>`).
    pub handle_endpoint: Option<String>,
}

impl PluginEnv {
    /// 환경변수에서 plugin 부트스트랩 정보를 읽는다. 필수 변수가 없으면 에러.
    pub fn load() -> Result<Self> {
        let plugin_id = std::env::var("TASTY_PLUGIN_ID")
            .map_err(|_| PluginError::EnvMissing("TASTY_PLUGIN_ID"))?;
        let port_raw = std::env::var("TASTY_HOST_IPC_PORT")
            .map_err(|_| PluginError::EnvMissing("TASTY_HOST_IPC_PORT"))?;
        let host_port = port_raw.parse::<u16>().map_err(|e| PluginError::EnvParse {
            var: "TASTY_HOST_IPC_PORT",
            message: e.to_string(),
        })?;
        let token = std::env::var("TASTY_PLUGIN_TOKEN")
            .map_err(|_| PluginError::EnvMissing("TASTY_PLUGIN_TOKEN"))?;
        let host_api_version =
            std::env::var("TASTY_HOST_API_VERSION").unwrap_or_else(|_| "1".to_string());
        Ok(Self {
            plugin_id,
            host_port,
            token,
            host_api_version,
            plugin_dir: std::env::var_os("TASTY_PLUGIN_DIR").map(PathBuf::from),
            data_dir: std::env::var_os("TASTY_PLUGIN_DATA_DIR").map(PathBuf::from),
            config_path: std::env::var_os("TASTY_PLUGIN_CONFIG_PATH").map(PathBuf::from),
            log_path: std::env::var_os("TASTY_PLUGIN_LOG_PATH").map(PathBuf::from),
            locale: std::env::var("TASTY_LOCALE").unwrap_or_else(|_| "en".to_string()),
            handle_endpoint: std::env::var("TASTY_PLUGIN_HANDLE_ENDPOINT")
                .ok()
                .filter(|s| !s.is_empty()),
        })
    }
}
