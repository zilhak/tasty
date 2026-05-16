//! `FileHandlerRegistry` — 등록된 핸들러들을 관리하고 detector 별로 정렬해 반환.
//!
//! 출처별 contribution 을 보관해 plugin uninstall 시 그 plugin 의 handler 만 제거.
//! 같은 handler id 가 여러 출처에 등장하면 patch semantics (Host → Plugin → User
//! 마지막 출처가 명시한 필드만 덮어씀).

use std::collections::BTreeMap;
use std::sync::RwLock;

use serde::Deserialize;
use tracing::warn;

use crate::file_format::DetectorId;

use super::config::{
    validate_plugin_handler_decl, HandlerDecl, HandlerDeclError, HostHandlerActionDecl,
    PluginHandlerActionDecl, UserHandlerActionDecl,
};
use super::types::{
    is_valid_handler_short_name, FileHandler, HandlerAction, HandlerId, HandlerOwner,
};

#[derive(Debug, Clone)]
struct HandlerContribution {
    owner: HandlerOwner,
    detector: Option<String>,
    priority: Option<i32>,
    display_name_i18n_key: Option<String>,
    disabled_override: Option<bool>,
    action: Option<HandlerAction>,
}

struct Inner {
    /// handler id → 출처별 contribution. install 순서 보존 (host → plugin → user).
    contributions: BTreeMap<HandlerId, Vec<HandlerContribution>>,
    finalized: BTreeMap<HandlerId, FileHandler>,
    dirty: bool,
}

pub struct FileHandlerRegistry {
    inner: RwLock<Inner>,
}

impl FileHandlerRegistry {
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(Inner {
                contributions: BTreeMap::new(),
                finalized: BTreeMap::new(),
                dirty: false,
            }),
        }
    }

    pub fn handler(&self, id: &HandlerId) -> Option<FileHandler> {
        self.ensure_finalized();
        let inner = self.inner.read().ok()?;
        inner.finalized.get(id).cloned()
    }

    pub fn list_handlers(&self) -> Vec<HandlerId> {
        self.ensure_finalized();
        let inner = match self.inner.read() {
            Ok(g) => g,
            Err(_) => return Vec::new(),
        };
        inner.finalized.keys().cloned().collect()
    }

    /// `detector` 에 attach 된 활성 handler 들. priority 오름차순, tie 시 owner 우선
    /// `user > plugin > host`, 그 후 handler id alphabetical.
    pub fn handlers_for(&self, detector: &DetectorId) -> Vec<FileHandler> {
        self.ensure_finalized();
        let inner = match self.inner.read() {
            Ok(g) => g,
            Err(_) => return Vec::new(),
        };
        let mut v: Vec<FileHandler> = inner
            .finalized
            .values()
            .filter(|h| !h.disabled && &h.detector == detector)
            .cloned()
            .collect();
        sort_handlers(&mut v);
        v
    }

    /// Picker modal 용 — 모든 enabled handler.
    pub fn all_handlers(&self) -> Vec<FileHandler> {
        self.ensure_finalized();
        let inner = match self.inner.read() {
            Ok(g) => g,
            Err(_) => return Vec::new(),
        };
        let mut v: Vec<FileHandler> = inner
            .finalized
            .values()
            .filter(|h| !h.disabled)
            .cloned()
            .collect();
        sort_handlers(&mut v);
        v
    }

    // ── install / uninstall ────────────────────────────────────────────

    pub fn install_host_defaults(&self, toml_text: &str) {
        let decls = match parse_host_handler_section(toml_text) {
            Ok(v) => v,
            Err(e) => {
                warn!(error = %e, "file_handler: failed to parse host defaults");
                return;
            }
        };
        let mut inner = match self.inner.write() {
            Ok(g) => g,
            Err(_) => return,
        };
        for decl in decls {
            install_host(&mut inner, decl);
        }
        inner.dirty = true;
    }

    pub fn install_user_config(&self, path: &std::path::Path) {
        let text = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
            Err(e) => {
                warn!(path = %path.display(), error = %e, "file_handler: user config read failed");
                return;
            }
        };
        let decls = match parse_user_handler_section(&text) {
            Ok(v) => v,
            Err(e) => {
                warn!(path = %path.display(), error = %e, "file_handler: user config parse failed");
                return;
            }
        };
        let mut inner = match self.inner.write() {
            Ok(g) => g,
            Err(_) => return,
        };
        for decl in decls {
            install_user(&mut inner, decl);
        }
        inner.dirty = true;
    }

    pub fn install_plugin_handlers(
        &self,
        plugin_id: &str,
        decls: &[HandlerDecl<PluginHandlerActionDecl>],
    ) {
        let mut inner = match self.inner.write() {
            Ok(g) => g,
            Err(_) => return,
        };
        for decl in decls {
            install_plugin(&mut inner, plugin_id, decl.clone());
        }
        inner.dirty = true;
    }

    pub fn uninstall_plugin(&self, plugin_id: &str) {
        let mut inner = match self.inner.write() {
            Ok(g) => g,
            Err(_) => return,
        };
        let mut empty_ids = Vec::new();
        for (id, contribs) in inner.contributions.iter_mut() {
            contribs.retain(|c| !matches!(&c.owner, HandlerOwner::Plugin(p) if p == plugin_id));
            if contribs.is_empty() {
                empty_ids.push(id.clone());
            }
        }
        for id in empty_ids {
            inner.contributions.remove(&id);
        }
        inner.dirty = true;
    }

    fn ensure_finalized(&self) {
        let needs = self
            .inner
            .read()
            .map(|g| g.dirty)
            .unwrap_or(false);
        if !needs {
            return;
        }
        let mut inner = match self.inner.write() {
            Ok(g) => g,
            Err(_) => return,
        };
        if !inner.dirty {
            return;
        }
        let mut next = BTreeMap::new();
        for (id, contribs) in inner.contributions.iter() {
            // 1번째 contribution 이 base — detector / action 필수
            let Some(base) = contribs.first() else { continue };
            let mut detector = base.detector.clone();
            let mut priority = base.priority;
            let mut display = base.display_name_i18n_key.clone();
            let mut disabled = base.disabled_override.unwrap_or(false);
            let mut action = base.action.clone();
            let mut owner = base.owner.clone();

            for c in contribs.iter().skip(1) {
                if c.detector.is_some() {
                    detector = c.detector.clone();
                }
                if c.priority.is_some() {
                    priority = c.priority;
                }
                if c.display_name_i18n_key.is_some() {
                    display = c.display_name_i18n_key.clone();
                }
                if let Some(d) = c.disabled_override {
                    disabled = d;
                }
                if c.action.is_some() {
                    action = c.action.clone();
                }
                // owner: 마지막 출처가 이김 (e.g. user override 시 user)
                owner = c.owner.clone();
            }

            // detector + action 둘 다 있어야 등록.
            let (Some(detector), Some(action)) = (detector, action) else {
                warn!(
                    handler_id = id.as_str(),
                    "file_handler: handler missing required detector or action — dropped"
                );
                continue;
            };
            let priority = priority.unwrap_or(100);
            next.insert(
                id.clone(),
                FileHandler {
                    id: id.clone(),
                    detector: DetectorId(detector),
                    priority,
                    owner,
                    action,
                    display_name_i18n_key: display,
                    disabled,
                },
            );
        }
        inner.finalized = next;
        inner.dirty = false;
    }
}

impl Default for FileHandlerRegistry {
    fn default() -> Self {
        Self::new()
    }
}

fn sort_handlers(v: &mut [FileHandler]) {
    v.sort_by(|a, b| {
        a.priority
            .cmp(&b.priority)
            .then_with(|| owner_rank(&a.owner).cmp(&owner_rank(&b.owner)))
            .then_with(|| a.id.cmp(&b.id))
    });
}

/// tie-break 시 owner 우선순위 — 작을수록 우선. `user > plugin > host`.
fn owner_rank(owner: &HandlerOwner) -> u8 {
    match owner {
        HandlerOwner::User => 0,
        HandlerOwner::Plugin(_) => 1,
        HandlerOwner::Host => 2,
    }
}

fn install_host(inner: &mut Inner, decl: HandlerDecl<HostHandlerActionDecl>) {
    let id_str = format!("host/{}", decl.id);
    if !is_valid_handler_short_name(&decl.id) {
        warn!(short_name = decl.id.as_str(), "file_handler: invalid host handler short-name");
        return;
    }
    push_contribution(
        inner,
        HandlerId(id_str),
        HandlerContribution {
            owner: HandlerOwner::Host,
            detector: Some(decl.detector),
            priority: Some(decl.priority),
            display_name_i18n_key: decl.display_name_i18n_key,
            disabled_override: if decl.disabled { Some(true) } else { None },
            action: Some(decl.action.into()),
        },
    );
}

fn install_plugin(
    inner: &mut Inner,
    plugin_id: &str,
    decl: HandlerDecl<PluginHandlerActionDecl>,
) {
    if let Err(e) = validate_plugin_handler_decl(&decl) {
        warn!(plugin = plugin_id, error = %e, "file_handler: rejecting plugin handler decl");
        return;
    }
    let id_str = format!("{}/{}", plugin_id, decl.id);
    let action = decl.action.into_handler_action(plugin_id);
    push_contribution(
        inner,
        HandlerId(id_str),
        HandlerContribution {
            owner: HandlerOwner::Plugin(plugin_id.to_string()),
            detector: Some(decl.detector),
            priority: Some(decl.priority),
            display_name_i18n_key: decl.display_name_i18n_key,
            disabled_override: if decl.disabled { Some(true) } else { None },
            action: Some(action),
        },
    );
    let _: Option<HandlerDeclError> = None; // suppress unused import if all paths Ok
}

fn install_user(inner: &mut Inner, decl: UserHandlerSettingsDecl) {
    // user TOML 의 id 는 전역 id 형태로 적힐 수 있다 (예: "host/markdown-viewer" disable).
    // 일반 user 신규 handler 는 "user/<short>" 형식.
    let id_str = decl.id.clone();
    let owner = if id_str.starts_with("user/") {
        HandlerOwner::User
    } else if id_str.starts_with("host/") {
        HandlerOwner::Host
    } else if let Some((prefix, _)) = id_str.split_once('/') {
        HandlerOwner::Plugin(prefix.to_string())
    } else {
        warn!(id = id_str.as_str(), "file_handler: user handler id missing owner prefix");
        return;
    };
    push_contribution(
        inner,
        HandlerId(id_str),
        HandlerContribution {
            owner,
            detector: decl.detector,
            priority: decl.priority,
            display_name_i18n_key: decl.display_name_i18n_key,
            disabled_override: decl.disabled,
            action: decl.action.map(Into::into),
        },
    );
}

fn push_contribution(inner: &mut Inner, id: HandlerId, contrib: HandlerContribution) {
    let entry = inner.contributions.entry(id).or_default();
    // 같은 origin 으로 재install 시 기존 동일 origin 제거 후 push.
    entry.retain(|c| !same_owner(&c.owner, &contrib.owner));
    entry.push(contrib);
}

fn same_owner(a: &HandlerOwner, b: &HandlerOwner) -> bool {
    match (a, b) {
        (HandlerOwner::Host, HandlerOwner::Host) => true,
        (HandlerOwner::User, HandlerOwner::User) => true,
        (HandlerOwner::Plugin(x), HandlerOwner::Plugin(y)) => x == y,
        _ => false,
    }
}

/// User TOML schema. 모든 필드 optional 로 patch 가능.
#[derive(Debug, Clone, Deserialize)]
struct UserHandlerSettingsDecl {
    id: String,
    #[serde(default)]
    detector: Option<String>,
    #[serde(default)]
    priority: Option<i32>,
    #[serde(default)]
    display_name_i18n_key: Option<String>,
    #[serde(default)]
    disabled: Option<bool>,
    #[serde(default)]
    action: Option<UserHandlerActionDecl>,
}

fn parse_host_handler_section(
    toml_text: &str,
) -> Result<Vec<HandlerDecl<HostHandlerActionDecl>>, toml::de::Error> {
    #[derive(Deserialize)]
    struct Wrap {
        #[serde(default, rename = "handler")]
        handlers: Vec<HandlerDecl<HostHandlerActionDecl>>,
    }
    let w: Wrap = toml::from_str(toml_text)?;
    Ok(w.handlers)
}

fn parse_user_handler_section(
    toml_text: &str,
) -> Result<Vec<UserHandlerSettingsDecl>, toml::de::Error> {
    #[derive(Deserialize)]
    struct Wrap {
        #[serde(default, rename = "handler")]
        handlers: Vec<UserHandlerSettingsDecl>,
    }
    let w: Wrap = toml::from_str(toml_text)?;
    Ok(w.handlers)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file_format::DetectorId;

    fn load_host(reg: &FileHandlerRegistry) {
        reg.install_host_defaults(include_str!("defaults/default-file-handlers.toml"));
    }

    #[test]
    fn host_default_loads_handlers_for_markdown() {
        let reg = FileHandlerRegistry::new();
        load_host(&reg);
        let v = reg.handlers_for(&DetectorId("markdown".into()));
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].id.as_str(), "host/markdown-viewer");
        matches!(v[0].action, HandlerAction::OpenSurface { .. });
    }

    #[test]
    fn plugin_handler_with_lower_priority_sorts_first() {
        let reg = FileHandlerRegistry::new();
        load_host(&reg);
        let decls = vec![HandlerDecl::<PluginHandlerActionDecl> {
            id: "viewer".into(),
            detector: "markdown".into(),
            priority: 10,
            display_name_i18n_key: None,
            disabled: false,
            action: PluginHandlerActionDecl::OpenSurface {
                surface_kind: "mdx_view".into(),
                param_key: "file".into(),
            },
        }];
        reg.install_plugin_handlers("com.example.mdx", &decls);
        let v = reg.handlers_for(&DetectorId("markdown".into()));
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].id.as_str(), "com.example.mdx/viewer");
        assert_eq!(v[1].id.as_str(), "host/markdown-viewer");
    }

    #[test]
    fn user_can_disable_host_handler() {
        let reg = FileHandlerRegistry::new();
        load_host(&reg);
        let user_toml = r#"
            [[handler]]
            id = "host/markdown-viewer"
            disabled = true
        "#;
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("file-handlers.toml");
        std::fs::write(&p, user_toml).unwrap();
        reg.install_user_config(&p);
        let v = reg.handlers_for(&DetectorId("markdown".into()));
        assert!(v.is_empty());
    }

    #[test]
    fn uninstall_plugin_removes_only_its_handlers() {
        let reg = FileHandlerRegistry::new();
        load_host(&reg);
        let decls = vec![HandlerDecl::<PluginHandlerActionDecl> {
            id: "viewer".into(),
            detector: "markdown".into(),
            priority: 10,
            display_name_i18n_key: None,
            disabled: false,
            action: PluginHandlerActionDecl::Ipc {
                method: "com.example.mdx.open".into(),
            },
        }];
        reg.install_plugin_handlers("com.example.mdx", &decls);
        assert_eq!(reg.handlers_for(&DetectorId("markdown".into())).len(), 2);
        reg.uninstall_plugin("com.example.mdx");
        let v = reg.handlers_for(&DetectorId("markdown".into()));
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].id.as_str(), "host/markdown-viewer");
    }

    #[test]
    fn handlers_for_priority_tiebreak_uses_owner_order() {
        let reg = FileHandlerRegistry::new();
        load_host(&reg);
        // plugin 과 user 모두 priority 50 (host 도 50)
        let p = vec![HandlerDecl::<PluginHandlerActionDecl> {
            id: "viewer".into(),
            detector: "markdown".into(),
            priority: 50,
            display_name_i18n_key: None,
            disabled: false,
            action: PluginHandlerActionDecl::Ipc {
                method: "com.example.x.open".into(),
            },
        }];
        reg.install_plugin_handlers("com.example.x", &p);
        let user_toml = r#"
            [[handler]]
            id = "user/my-viewer"
            detector = "markdown"
            priority = 50
            [handler.action]
            kind = "system"
        "#;
        let dir = tempfile::tempdir().unwrap();
        let pth = dir.path().join("file-handlers.toml");
        std::fs::write(&pth, user_toml).unwrap();
        reg.install_user_config(&pth);

        let v = reg.handlers_for(&DetectorId("markdown".into()));
        // priority 모두 50 → tie-break: user > plugin > host
        let owners: Vec<&str> = v.iter().map(|h| h.id.as_str()).collect();
        assert_eq!(owners[0], "user/my-viewer");
        assert_eq!(owners[1], "com.example.x/viewer");
        assert_eq!(owners[2], "host/markdown-viewer");
    }

    #[test]
    fn all_handlers_returns_every_enabled() {
        let reg = FileHandlerRegistry::new();
        load_host(&reg);
        let v = reg.all_handlers();
        // host default 4개
        assert_eq!(v.len(), 4);
    }
}
