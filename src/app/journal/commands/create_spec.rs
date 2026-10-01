//! Application inputs for kind materialization. Surface IDs are targets, never focus aliases.
use crate::app::command::{ConvertSurfaceTarget, DomainIntent};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) enum Destination {
    Adopt {
        pane: u32,
        pty: u32,
    },
    Workspace {
        name: Option<String>,
        subtitle: Option<String>,
        description: Option<String>,
        category: Option<u32>,
    },
    Tab {
        pane: u32,
        name: Option<String>,
        activate: bool,
    },
    Pane {
        target: u32,
        direction: crate::model::SplitDirection,
    },
    Surface {
        target: u32,
        direction: crate::model::SplitDirection,
    },
    Convert {
        surface: u32,
        respawn: bool,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct Spec {
    pub destination: Destination,
    pub kind: String,
    pub cwd: Option<std::path::PathBuf>,
    pub params: serde_json::Value,
}

impl Spec {
    pub fn from_intent(intent: &DomainIntent) -> Option<Self> {
        let (destination, kind, cwd, params) = match intent {
            DomainIntent::AdoptTerminal { pane_id, pty_id } => (
                Destination::Adopt {
                    pane: *pane_id,
                    pty: *pty_id,
                },
                "terminal".into(),
                None,
                serde_json::json!({}),
            ),
            DomainIntent::CreateWorkspace {
                cwd,
                kind,
                surface_params,
                name,
                subtitle,
                description,
                category,
            } => (
                Destination::Workspace {
                    name: name.clone(),
                    subtitle: subtitle.clone(),
                    description: description.clone(),
                    category: *category,
                },
                kind.clone(),
                cwd.clone(),
                surface_params.clone(),
            ),
            DomainIntent::CreateTab {
                pane_id,
                cwd,
                kind,
                name,
                surface_params,
                activate,
            } => (
                Destination::Tab {
                    pane: *pane_id,
                    name: name.clone(),
                    activate: *activate,
                },
                kind.clone(),
                cwd.clone(),
                surface_params.clone(),
            ),
            DomainIntent::SplitPane {
                target_pane_id,
                direction,
                cwd,
                kind,
                surface_params,
            } => (
                Destination::Pane {
                    target: *target_pane_id,
                    direction: *direction,
                },
                kind.clone(),
                cwd.clone(),
                surface_params.clone(),
            ),
            DomainIntent::SplitSurface {
                target_surface_id,
                direction,
                cwd,
                kind,
                surface_params,
            } => (
                Destination::Surface {
                    target: *target_surface_id,
                    direction: *direction,
                },
                kind.clone(),
                cwd.clone(),
                surface_params.clone(),
            ),
            DomainIntent::ConvertSurface { surface_id, target } => {
                let (kind, cwd, params) = match target {
                    ConvertSurfaceTarget::Terminal { cwd } => {
                        ("terminal".into(), cwd.clone(), serde_json::json!({}))
                    }
                    ConvertSurfaceTarget::Kind { cwd, kind, params } => {
                        (kind.clone(), cwd.clone(), params.clone())
                    }
                };
                (
                    Destination::Convert {
                        surface: *surface_id,
                        respawn: false,
                    },
                    kind,
                    cwd,
                    params,
                )
            }
            DomainIntent::RespawnTerminal { surface_id, cwd } => (
                Destination::Convert {
                    surface: *surface_id,
                    respawn: true,
                },
                "terminal".into(),
                cwd.clone(),
                serde_json::json!({}),
            ),
            _ => return None,
        };
        Some(Self {
            destination,
            kind,
            cwd,
            params,
        })
    }
}

impl Spec {
    pub fn from_public(
        request: &crate::ipc::protocol::JsonRpcRequest,
        session: &crate::runtime::engine_session::EngineSession,
        view: &crate::runtime::journal_product::CompletionView,
        services: &crate::app::services::AppServices,
    ) -> Result<Self, crate::ipc::protocol::JsonRpcResponse> {
        use crate::ipc::{handler::params, protocol::JsonRpcResponse};
        let id = serde_json::Value::Null;
        let bad = |message: String| JsonRpcResponse::invalid_params(id.clone(), message);
        let engine = session.as_ref();
        let mut input = request.params.clone();
        let mut kind = input
            .get("type")
            .and_then(|value| value.as_str())
            .unwrap_or("terminal")
            .to_owned();
        let explicit_cwd = input
            .get("cwd")
            .and_then(|value| value.as_str())
            .map(std::path::PathBuf::from);
        let selected = |pane: u32| {
            engine
                .find_pane_by_id(pane)
                .and_then(|pane| {
                    view.selected_tabs
                        .get(&pane.id)
                        .and_then(|id| pane.tabs.iter().find(|tab| tab.id == *id))
                        .or_else(|| pane.tabs.first())
                })
                .and_then(|tab| {
                    view.selected_surfaces
                        .get(&tab.id)
                        .copied()
                        .filter(|id| tab.contains_surface(*id))
                        .or_else(|| tab.first_surface_id())
                })
        };
        let inherit = |surface: Option<u32>| {
            if engine.runtime.settings.general.inherit_cwd {
                surface.and_then(|surface| engine.local_surface_cwd(surface))
            } else {
                None
            }
        };
        let (destination, cwd) = match request.method.as_str() {
            "image.open" => {
                let surface = params::require_u32(&input, "surface_id", &id)?;
                let path = input
                    .get("path")
                    .and_then(|value| value.as_str())
                    .ok_or_else(|| bad("Missing required 'path' parameter".into()))?
                    .to_owned();
                kind = "image".into();
                input = serde_json::json!({"file":path});
                (
                    Destination::Convert {
                        surface,
                        respawn: false,
                    },
                    None,
                )
            }
            "pty.attach_surface" => {
                kind = "terminal".into();
                (
                    Destination::Adopt {
                        pane: params::require_u32(&input, "pane_id", &id)?,
                        pty: params::require_u32(&input, "id", &id)?,
                    },
                    None,
                )
            }
            "tab.create" => {
                let pane = params::require_u32(&input, "pane_id", &id)?;
                if engine.find_pane_by_id(pane).is_none() {
                    return Err(bad(format!("Pane {pane} not found")));
                }
                if let Some(definition) = engine.runtime.surface_registry.get(&kind) {
                    let home =
                        directories::BaseDirs::new().map(|dirs| dirs.home_dir().to_path_buf());
                    engine.apply_kind_default_params(&definition, &mut input, home.as_deref());
                }
                let cwd = if kind == "terminal" {
                    explicit_cwd.or_else(|| inherit(selected(pane)))
                } else {
                    None
                };
                (
                    Destination::Tab {
                        pane,
                        name: input
                            .get("name")
                            .and_then(|value| value.as_str())
                            .map(str::to_owned),
                        activate: false,
                    },
                    cwd,
                )
            }
            "split" => {
                let surface = crate::ipc::handler::pane::resolve_surface_target(services, &input);
                let pane = params::optional_u32(&input, "target_pane", &id)?;
                if surface.is_none() && pane.is_none() {
                    return Err(bad("Missing target. Use 'target_surface' (surface ID or nickname) and/or 'target_pane' (pane ID)".into()));
                }
                if surface.is_some() && pane.is_some() {
                    return Err(bad(
                        "Cannot specify both 'target_surface' and 'target_pane'. Use one.".into(),
                    ));
                }
                if let Some(path) = &explicit_cwd
                    && !path.is_dir()
                {
                    return Err(bad(format!("cwd does not exist: {}", path.display())));
                }
                if let Some(definition) = engine.runtime.surface_registry.get_live(&kind)
                    && let Some(missing) = definition.first_missing_required_param(&input)
                {
                    return Err(bad(format!(
                        "Missing '{missing}' parameter for {kind} type"
                    )));
                }
                let direction = match input.get("direction").and_then(|value| value.as_str()) {
                    Some("horizontal" | "h") => crate::model::SplitDirection::Horizontal,
                    _ => crate::model::SplitDirection::Vertical,
                };
                match input.get("level").and_then(|value| value.as_str()) {
                    Some("pane" | "pane-group") => {
                        let target = pane
                            .or_else(|| {
                                surface.and_then(|surface| engine.find_pane_for_surface(surface))
                            })
                            .ok_or_else(|| {
                                bad(format!("Surface {} not found", surface.unwrap_or(0)))
                            })?;
                        (
                            Destination::Pane { target, direction },
                            if kind == "terminal" {
                                explicit_cwd.or_else(|| inherit(selected(target)))
                            } else {
                                None
                            },
                        )
                    }
                    Some("surface") => {
                        let target = surface.ok_or_else(|| {
                            bad(
                                "Surface-level split requires 'target_surface', not 'target_pane'"
                                    .into(),
                            )
                        })?;
                        (
                            Destination::Surface { target, direction },
                            if kind == "terminal" {
                                explicit_cwd.or_else(|| inherit(Some(target)))
                            } else {
                                None
                            },
                        )
                    }
                    Some(other) => {
                        return Err(bad(format!("Invalid level '{other}'. Use: pane, surface")));
                    }
                    None => return Err(bad("Missing 'level' parameter".into())),
                }
            }
            "surface.respawn_terminal" => {
                kind = "terminal".into();
                let surface = params::require_u32(&input, "surface_id", &id)?;
                if engine.runtime.terminals.get(surface).is_none() {
                    return Err(bad(format!("Surface {surface} is not a terminal")));
                }
                (
                    Destination::Convert {
                        surface,
                        respawn: true,
                    },
                    explicit_cwd,
                )
            }
            _ => return Err(bad("unsupported kind creation method".into())),
        };
        if let Some(path) = &cwd
            && !path.is_dir()
        {
            return Err(bad(format!("cwd does not exist: {}", path.display())));
        }
        Ok(Self {
            destination,
            kind,
            cwd,
            params: input,
        })
    }
}
