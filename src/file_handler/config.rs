//! Handler TOML schema. Actor 별 action variant 차이를 schema 에서 강제한다.
//!
//! - `HostHandlerActionDecl`: `OpenSurface` / `Ipc` / `System`
//! - `PluginHandlerActionDecl`: `OpenSurface` / `Ipc` (System 없음 — manifest reject)
//! - `UserHandlerActionDecl`: `OpenSurface` / `Ipc` / `System`

use serde::Deserialize;

fn default_param_key() -> String {
    "file".to_string()
}

/// Handler 정의의 actor-agnostic 표면.
#[derive(Debug, Clone, Deserialize)]
pub struct HandlerDecl<A> {
    /// short-name. 전역 id 로 합쳐질 때 `<owner_prefix>/<short-name>` 이 된다.
    pub id: String,
    pub detector: String,
    pub priority: i32,
    #[serde(default)]
    pub display_name_i18n_key: Option<String>,
    #[serde(default)]
    pub disabled: bool,
    pub action: A,
}

/// Host default 가 사용 가능한 action set.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HostHandlerActionDecl {
    OpenSurface {
        surface_kind: String,
        #[serde(default = "default_param_key")]
        param_key: String,
    },
    Ipc {
        method: String,
    },
    System,
}

/// Plugin manifest 가 사용 가능한 action set. **`System` variant 없음** —
/// manifest 에 `kind = "system"` 적으면 serde unknown variant 로 reject.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PluginHandlerActionDecl {
    OpenSurface {
        surface_kind: String,
        #[serde(default = "default_param_key")]
        param_key: String,
    },
    Ipc {
        method: String,
    },
}

/// User config 가 사용 가능한 action set.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UserHandlerActionDecl {
    OpenSurface {
        surface_kind: String,
        #[serde(default = "default_param_key")]
        param_key: String,
    },
    Ipc {
        method: String,
    },
    System,
}
