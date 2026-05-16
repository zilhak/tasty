//! Handler TOML schema. Actor 별 action variant 차이를 schema 에서 강제한다.
//!
//! - `HostHandlerActionDecl`: `OpenSurface` / `Ipc` / `System`
//! - `PluginHandlerActionDecl`: `OpenSurface` / `Ipc` (System 없음 — manifest reject)
//! - `UserHandlerActionDecl`: `OpenSurface` / `Ipc` / `System`

use std::fmt;

use serde::Deserialize;

use super::types::{is_valid_handler_short_name, HandlerAction};

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

impl From<HostHandlerActionDecl> for HandlerAction {
    fn from(d: HostHandlerActionDecl) -> Self {
        match d {
            HostHandlerActionDecl::OpenSurface { surface_kind, param_key } => {
                HandlerAction::OpenSurface { surface_kind, param_key }
            }
            HostHandlerActionDecl::Ipc { method } => HandlerAction::Ipc {
                method,
                owner_plugin_id: String::new(), // host IPC 는 1단계 미사용. 실 호출 시 검증.
            },
            HostHandlerActionDecl::System => HandlerAction::System,
        }
    }
}

/// Plugin decl 은 owner plugin id 가 외부에서 주입되어야 한다 (manifest 의 `id`).
impl PluginHandlerActionDecl {
    pub fn into_handler_action(self, owner_plugin_id: &str) -> HandlerAction {
        match self {
            PluginHandlerActionDecl::OpenSurface { surface_kind, param_key } => {
                HandlerAction::OpenSurface { surface_kind, param_key }
            }
            PluginHandlerActionDecl::Ipc { method } => HandlerAction::Ipc {
                method,
                owner_plugin_id: owner_plugin_id.to_string(),
            },
        }
    }
}

impl From<UserHandlerActionDecl> for HandlerAction {
    fn from(d: UserHandlerActionDecl) -> Self {
        match d {
            UserHandlerActionDecl::OpenSurface { surface_kind, param_key } => {
                HandlerAction::OpenSurface { surface_kind, param_key }
            }
            UserHandlerActionDecl::Ipc { method } => HandlerAction::Ipc {
                method,
                owner_plugin_id: String::new(), // user 영역은 method 만으로 plugin 추적
            },
            UserHandlerActionDecl::System => HandlerAction::System,
        }
    }
}

/// Handler decl schema 검증.
#[derive(Debug, Clone)]
pub enum HandlerDeclError {
    InvalidShortName(String),
    DetectorIsUnknownSentinel(String),
    InvalidDetectorId { handler: String, detector: String },
    OpenSurfaceSurfaceKindUnknown { handler: String, surface_kind: String },
    IpcMethodOutsideOwnNamespace { handler: String, method: String },
}

impl fmt::Display for HandlerDeclError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidShortName(s) => write!(
                f,
                "invalid handler short-name '{s}' (must match [a-z0-9-]{{1,32}})"
            ),
            Self::DetectorIsUnknownSentinel(h) => write!(
                f,
                "handler '{h}': detector = '$unknown' is not allowed — there is no such detector"
            ),
            Self::InvalidDetectorId { handler, detector } => write!(
                f,
                "handler '{handler}': invalid detector id '{detector}'"
            ),
            Self::OpenSurfaceSurfaceKindUnknown { handler, surface_kind } => write!(
                f,
                "handler '{handler}': surface_kind '{surface_kind}' is not declared by this plugin"
            ),
            Self::IpcMethodOutsideOwnNamespace { handler, method } => write!(
                f,
                "handler '{handler}': ipc method '{method}' must start with this plugin's namespace prefix"
            ),
        }
    }
}

impl std::error::Error for HandlerDeclError {}

/// Plugin 단독 schema 검증 (cross-ref 는 외부 별 함수).
pub fn validate_plugin_handler_decl(
    decl: &HandlerDecl<PluginHandlerActionDecl>,
) -> Result<(), HandlerDeclError> {
    use crate::file_format::is_valid_detector_id;

    if !is_valid_handler_short_name(&decl.id) {
        return Err(HandlerDeclError::InvalidShortName(decl.id.clone()));
    }
    if decl.detector == "$unknown" {
        return Err(HandlerDeclError::DetectorIsUnknownSentinel(decl.id.clone()));
    }
    if !is_valid_detector_id(&decl.detector) {
        return Err(HandlerDeclError::InvalidDetectorId {
            handler: decl.id.clone(),
            detector: decl.detector.clone(),
        });
    }
    Ok(())
}

/// Plugin handler 의 cross-ref 검증. 같은 plugin 의 `[[surface_kinds]]` /
/// `[[contributes.ipc_namespace]]` prefix 와 매칭되어야 한다.
pub fn validate_plugin_handler_refs(
    decl: &HandlerDecl<PluginHandlerActionDecl>,
    plugin_surface_kinds: &[String],
    plugin_ipc_prefixes: &[String],
) -> Result<(), HandlerDeclError> {
    match &decl.action {
        PluginHandlerActionDecl::OpenSurface { surface_kind, .. } => {
            if !plugin_surface_kinds.iter().any(|k| k == surface_kind) {
                return Err(HandlerDeclError::OpenSurfaceSurfaceKindUnknown {
                    handler: decl.id.clone(),
                    surface_kind: surface_kind.clone(),
                });
            }
        }
        PluginHandlerActionDecl::Ipc { method } => {
            let ok = plugin_ipc_prefixes.iter().any(|p| {
                method == p || method.starts_with(&format!("{p}."))
            });
            if !ok {
                return Err(HandlerDeclError::IpcMethodOutsideOwnNamespace {
                    handler: decl.id.clone(),
                    method: method.clone(),
                });
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Deserialize, Debug)]
    struct PluginWrap {
        #[serde(rename = "handler")]
        handlers: Vec<HandlerDecl<PluginHandlerActionDecl>>,
    }

    fn parse_plugin(s: &str) -> Result<PluginWrap, toml::de::Error> {
        toml::from_str(s)
    }

    #[derive(Deserialize, Debug)]
    struct HostWrap {
        #[serde(rename = "handler")]
        handlers: Vec<HandlerDecl<HostHandlerActionDecl>>,
    }

    fn parse_host(s: &str) -> Result<HostWrap, toml::de::Error> {
        toml::from_str(s)
    }

    #[test]
    fn host_can_use_system_kind() {
        let t = r#"
            [[handler]]
            id = "directory-system"
            detector = "$directory"
            priority = 50
            [handler.action]
            kind = "system"
        "#;
        let h = parse_host(t).expect("host parse");
        assert_eq!(h.handlers.len(), 1);
        matches!(h.handlers[0].action, HostHandlerActionDecl::System);
    }

    #[test]
    fn plugin_system_kind_rejected() {
        let t = r#"
            [[handler]]
            id = "x"
            detector = "pdf"
            priority = 1
            [handler.action]
            kind = "system"
        "#;
        let err = parse_plugin(t).expect_err("plugin must reject system");
        // serde 의 unknown variant 메시지 (정확한 문자열 의존 X — Err 인지만 확인)
        assert!(format!("{err}").to_lowercase().contains("system")
            || format!("{err}").to_lowercase().contains("unknown variant"));
    }

    #[test]
    fn plugin_open_surface_parses() {
        let t = r#"
            [[handler]]
            id = "viewer"
            detector = "pdf"
            priority = 100
            [handler.action]
            kind = "open_surface"
            surface_kind = "pdf_view"
            param_key = "file"
        "#;
        let h = parse_plugin(t).expect("parse");
        assert_eq!(h.handlers[0].id, "viewer");
        match &h.handlers[0].action {
            PluginHandlerActionDecl::OpenSurface { surface_kind, param_key } => {
                assert_eq!(surface_kind, "pdf_view");
                assert_eq!(param_key, "file");
            }
            _ => panic!("expected OpenSurface"),
        }
    }

    #[test]
    fn validate_rejects_unknown_sentinel() {
        let decl = HandlerDecl::<PluginHandlerActionDecl> {
            id: "x".into(),
            detector: "$unknown".into(),
            priority: 1,
            display_name_i18n_key: None,
            disabled: false,
            action: PluginHandlerActionDecl::Ipc {
                method: "x.open".into(),
            },
        };
        assert!(matches!(
            validate_plugin_handler_decl(&decl),
            Err(HandlerDeclError::DetectorIsUnknownSentinel(_))
        ));
    }

    #[test]
    fn validate_rejects_bad_short_name() {
        let decl = HandlerDecl::<PluginHandlerActionDecl> {
            id: "Bad/Name".into(),
            detector: "pdf".into(),
            priority: 1,
            display_name_i18n_key: None,
            disabled: false,
            action: PluginHandlerActionDecl::Ipc {
                method: "x.open".into(),
            },
        };
        assert!(matches!(
            validate_plugin_handler_decl(&decl),
            Err(HandlerDeclError::InvalidShortName(_))
        ));
    }

    #[test]
    fn cross_ref_open_surface_unknown_kind() {
        let decl = HandlerDecl::<PluginHandlerActionDecl> {
            id: "viewer".into(),
            detector: "pdf".into(),
            priority: 1,
            display_name_i18n_key: None,
            disabled: false,
            action: PluginHandlerActionDecl::OpenSurface {
                surface_kind: "other_plugin_view".into(),
                param_key: "file".into(),
            },
        };
        let res = validate_plugin_handler_refs(&decl, &[], &[]);
        assert!(matches!(
            res,
            Err(HandlerDeclError::OpenSurfaceSurfaceKindUnknown { .. })
        ));
    }

    #[test]
    fn cross_ref_ipc_outside_namespace() {
        let decl = HandlerDecl::<PluginHandlerActionDecl> {
            id: "viewer".into(),
            detector: "pdf".into(),
            priority: 1,
            display_name_i18n_key: None,
            disabled: false,
            action: PluginHandlerActionDecl::Ipc {
                method: "other.method".into(),
            },
        };
        let res = validate_plugin_handler_refs(&decl, &[], &["mine".into()]);
        assert!(matches!(
            res,
            Err(HandlerDeclError::IpcMethodOutsideOwnNamespace { .. })
        ));
    }

    #[test]
    fn cross_ref_ipc_inside_namespace_ok() {
        let decl = HandlerDecl::<PluginHandlerActionDecl> {
            id: "viewer".into(),
            detector: "pdf".into(),
            priority: 1,
            display_name_i18n_key: None,
            disabled: false,
            action: PluginHandlerActionDecl::Ipc {
                method: "mine.open".into(),
            },
        };
        assert!(validate_plugin_handler_refs(&decl, &[], &["mine".into()]).is_ok());
    }

    #[test]
    fn plugin_decl_to_handler_action_carries_owner() {
        let decl = PluginHandlerActionDecl::Ipc {
            method: "mine.open".into(),
        };
        match decl.into_handler_action("com.example.x") {
            HandlerAction::Ipc { owner_plugin_id, method } => {
                assert_eq!(owner_plugin_id, "com.example.x");
                assert_eq!(method, "mine.open");
            }
            _ => panic!("expected Ipc"),
        }
    }
}
