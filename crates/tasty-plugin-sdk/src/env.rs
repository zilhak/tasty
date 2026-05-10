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

use std::path::PathBuf;

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
}

impl PluginEnv {
    /// 환경변수에서 plugin 부트스트랩 정보를 읽는다. 필수 변수가 없으면 에러.
    pub fn load() -> anyhow::Result<Self> {
        let plugin_id = std::env::var("TASTY_PLUGIN_ID")
            .map_err(|_| anyhow::anyhow!("TASTY_PLUGIN_ID env var missing"))?;
        let host_port = std::env::var("TASTY_HOST_IPC_PORT")
            .map_err(|_| anyhow::anyhow!("TASTY_HOST_IPC_PORT env var missing"))?
            .parse::<u16>()
            .map_err(|e| anyhow::anyhow!("TASTY_HOST_IPC_PORT parse error: {e}"))?;
        let token = std::env::var("TASTY_PLUGIN_TOKEN")
            .map_err(|_| anyhow::anyhow!("TASTY_PLUGIN_TOKEN env var missing"))?;
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
        })
    }
}
