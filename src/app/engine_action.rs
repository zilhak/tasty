//! Non-journal execution requests carry targets observed by a View, never a mutable Engine borrow.
use crate::runtime::engine_access::{EngineMut, EngineRef};
#[derive(Clone, Debug)]
pub(crate) struct SurfaceBinding {
    surface: u32,
    activation: Option<u64>,
    resource: Option<tasty_terminal::ResourceGeneration>,
    mirror: Option<(u32, std::sync::Weak<()>)>,
}
impl SurfaceBinding {
    pub(crate) fn capture(
        engine: &crate::runtime::engine_read::EngineRead<'_>,
        surface: u32,
    ) -> Option<Self> {
        let descriptor = engine.core.find_surface_by_id(surface)?;
        let mirror = engine
            .find_workspace_index_for_surface(surface)
            .and_then(|(index, _)| engine.workspace_at(index))
            .filter(|workspace| workspace.mirror)
            .and_then(|workspace| {
                engine
                    .mirror_projection_token(workspace.id)
                    .map(|token| (workspace.id, token))
            });
        Some(Self {
            surface,
            activation: descriptor.activation_generation,
            resource: engine.terminals.generation(surface),
            mirror,
        })
    }
    #[cfg(feature = "gui")]
    pub(crate) fn take_mouse_capture_hint(
        &self,
        engine: &mut EngineMut<'_>,
        foreground_generation: u64,
    ) -> Option<u32> {
        if !self.current(&engine.as_ref())
            || engine.foreground_generation(self.surface) != foreground_generation
            || !engine.runtime.settings.general.mouse_capture_hint
            || engine.foreground_name(self.surface).is_some_and(|name| {
                engine
                    .runtime
                    .settings
                    .general
                    .mouse_capture_banner_disabled_for(name)
            })
        {
            return None;
        }
        engine
            .runtime
            .terminals
            .get_mut(self.surface)?
            .take_mouse_capture_hint()
            .then_some(self.surface)
    }
    pub(crate) fn current(&self, engine: &EngineRef<'_>) -> bool {
        engine
            .core
            .find_surface_by_id(self.surface)
            .is_some_and(|descriptor| descriptor.activation_generation == self.activation)
            && self.resource.is_none_or(|generation| {
                engine
                    .runtime
                    .terminals
                    .matches_generation(self.surface, generation)
            })
            && self.mirror.as_ref().is_none_or(|(workspace, token)| {
                engine.matches_mirror_projection(*workspace, token)
            })
    }
}
#[derive(Clone, Copy, Debug)]
pub(crate) enum ZoomChange {
    In,
    Out,
    Reset,
}

#[derive(Clone, Debug)]
pub(crate) enum SettingsPatch {
    PluginZoom {
        plugin: String,
        change: ZoomChange,
    },
    FontSize {
        kind: Option<String>,
        change: ZoomChange,
    },
    ExplorerMode(String),
    SuppressMouseHint(String),
    DisableMouseCapture(String),
}
impl SettingsPatch {
    pub(crate) fn apply(&self, settings: &mut crate::settings::Settings) {
        match self {
            Self::PluginZoom { plugin, change } => {
                use crate::settings::PluginSettingValue;
                let current = match settings.plugin_setting(plugin, "zoom") {
                    Some(PluginSettingValue::Number(value)) => *value,
                    _ => 100.0,
                };
                let value = match change {
                    ZoomChange::In => (current + 10.0).min(500.0),
                    ZoomChange::Out => (current - 10.0).max(25.0),
                    ZoomChange::Reset => 100.0,
                };
                settings.set_plugin_setting(plugin, "zoom", PluginSettingValue::Number(value));
            }
            Self::FontSize { kind, change } => {
                let appearance = &mut settings.appearance;
                let current = match kind {
                    Some(kind) => appearance.effective_font_for_kind(kind).font_size,
                    None => appearance.effective_terminal_font().font_size,
                };
                let size = match change {
                    ZoomChange::In => Some((current + 1.0).min(72.0)),
                    ZoomChange::Out => Some((current - 1.0).max(6.0)),
                    ZoomChange::Reset => None,
                };
                let font = match kind {
                    Some(kind) => appearance
                        .plugin_font_overrides
                        .entry(kind.clone())
                        .or_default(),
                    None => &mut appearance.terminal_font,
                };
                font.font_size = size;
            }
            Self::ExplorerMode(mode) => settings.general.explorer_view_mode = mode.clone(),
            Self::SuppressMouseHint(name) => settings
                .general
                .mouse_capture_banner_blacklist
                .push(name.clone()),
            Self::DisableMouseCapture(name) => {
                settings.general.mouse_capture_blacklist.push(name.clone())
            }
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) enum EngineAction {
    #[cfg(feature = "gui")]
    Html(super::html_runtime::HtmlAction),
    #[cfg(feature = "gui")]
    PluginDisplay(super::plugin_display::PluginDisplayRequest),
    #[cfg(feature = "gui")]
    PasteImage {
        target: SurfaceBinding,
        view: std::sync::Weak<()>,
        bracketed: bool,
        file_name: String,
        png_bytes: Vec<u8>,
    },
    #[cfg(feature = "gui")]
    ImageUpload {
        target: SurfaceBinding,
        request: crate::core::PendingImageUpload,
    },
    #[cfg(feature = "gui")]
    Screenshot {
        target: Option<SurfaceBinding>,
        mirror_workspace: Option<u32>,
        view: std::sync::Weak<()>,
    },
    #[cfg(feature = "gui")]
    AttachUser(AttachRequest),
    #[cfg(feature = "gui")]
    Explorer {
        target: SurfaceBinding,
        action: crate::explorer_ui::ExplorerAction,
    },
    #[cfg(feature = "gui")]
    DagSelection {
        target: SurfaceBinding,
        dag_id: Option<String>,
        direction: crate::model::DagDirection,
    },
    RenameExplorerEntry {
        target: SurfaceBinding,
        path: std::path::PathBuf,
        name: String,
    },
    #[cfg(feature = "gui")]
    FocusObserved {
        target: SurfaceBinding,
    },
    RecordTyping {
        target: SurfaceBinding,
        at: std::time::Instant,
    },
    ExplorerCwd {
        target: SurfaceBinding,
        folder: std::path::PathBuf,
    },
    #[cfg(feature = "gui")]
    RemoveExplorerFavorite {
        path: std::path::PathBuf,
    },
    #[cfg(feature = "gui")]
    AddExplorerFavorite {
        path: std::path::PathBuf,
        label: String,
    },
    #[cfg(feature = "gui")]
    TogglePortFavorite {
        address: std::net::IpAddr,
        port: u16,
        label: String,
    },
    #[cfg(feature = "gui")]
    DetachSurface {
        surface: u32,
        grant: u64,
    },
    DetachWorkspace {
        workspace: u32,
        holder: u32,
        grant: u64,
    },
    #[cfg(feature = "gui")]
    RemoteMeshFull {
        targets: Vec<SurfaceBinding>,
    },
    #[cfg(feature = "gui")]
    ListDirectory {
        request: crate::core::PendingListDirForward,
        projection: std::sync::Weak<()>,
        target: Option<SurfaceBinding>,
    },
    DefaultGrid {
        cols: usize,
        rows: usize,
    },
    Resize {
        targets: Vec<(SurfaceBinding, usize, usize)>,
    },
    #[cfg(feature = "gui")]
    LocalMesh {
        target: SurfaceBinding,
        plugin: String,
        registration: crate::runtime::kind_catalog::Registration,
        bootstrap: Option<(String, Option<String>, String)>,
        params: tasty_plugin_protocol::protocol::SurfaceSetContextParams,
    },
    #[cfg(feature = "gui")]
    RemoteMesh {
        target: SurfaceBinding,
        context: Option<crate::core::state::AttachMeshContextForward>,
        input: Option<tasty_plugin_protocol::protocol::RawInputWire>,
    },
}
impl EngineAction {
    pub(crate) fn apply(
        &self,
        engine: &mut EngineMut<'_>,
        plugins: Option<&crate::plugin::PluginManager>,
    ) {
        match self {
            #[cfg(feature = "gui")]
            Self::Html(action) => action.apply(engine),
            #[cfg(feature = "gui")]
            Self::PluginDisplay(request) => {
                if let Some(manager) = plugins {
                    request.apply(manager);
                }
            }
            #[cfg(feature = "gui")]
            Self::AttachUser(request) => {
                if let Some(request) = request.take() {
                    engine.remote.pending_gui_attach_user.push(request);
                }
            }
            #[cfg(feature = "gui")]
            Self::Explorer { target, action } => {
                if target.current(&engine.as_ref())
                    && let Some(panel) =
                        engine
                            .runtime
                            .surfaces
                            .get_mut(&target.surface)
                            .and_then(|surface| {
                                surface
                                    .as_any_mut()
                                    .downcast_mut::<crate::model::ExplorerPanel>()
                            })
                {
                    super::explorer_action::apply_to_explorer_panel(panel, action);
                    engine.mark_layout_dirty();
                }
            }
            #[cfg(feature = "gui")]
            Self::DagSelection {
                target,
                dag_id,
                direction,
            } => {
                if target.current(&engine.as_ref())
                    && let Some(dag) =
                        engine
                            .runtime
                            .surfaces
                            .get_mut(&target.surface)
                            .and_then(|surface| {
                                surface
                                    .as_any_mut()
                                    .downcast_mut::<crate::model::DagGraphSurface>()
                            })
                {
                    dag.dag_id = dag_id.clone();
                    dag.direction = *direction;
                    engine.mark_layout_dirty();
                }
            }
            Self::RenameExplorerEntry { target, path, name } => {
                if target.current(&engine.as_ref())
                    && let Some(parent) = path.parent()
                {
                    let next = parent.join(name);
                    if next != *path
                        && let Err(error) = std::fs::rename(path, &next)
                    {
                        tracing::warn!(%error,"explorer rename failed");
                    }
                }
            }
            #[cfg(feature = "gui")]
            Self::Screenshot {
                target,
                mirror_workspace,
                view,
            } => {
                if target
                    .as_ref()
                    .is_none_or(|target| target.current(&engine.as_ref()))
                {
                    engine
                        .remote
                        .pending_screenshot_captures
                        .push((*mirror_workspace, view.clone()));
                }
            }
            #[cfg(feature = "gui")]
            Self::ImageUpload { target, request } => {
                if target.current(&engine.as_ref()) {
                    engine.remote.pending_image_uploads.push(request.clone());
                }
            }
            #[cfg(feature = "gui")]
            Self::PasteImage {
                target,
                view,
                bracketed,
                file_name,
                png_bytes,
            } => {
                if !target.current(&engine.as_ref()) {
                    return;
                }
                if let Some((workspace, _)) = &target.mirror {
                    engine
                        .remote
                        .pending_image_uploads
                        .push(crate::core::PendingImageUpload {
                            origin_view: view.clone(),
                            mirror_ws_id: *workspace,
                            surface_id: target.surface,
                            bracketed: *bracketed,
                            file_name: file_name.clone(),
                            png_bytes: png_bytes.clone(),
                        });
                } else {
                    let directory = std::env::temp_dir().join("tasty-clipboard");
                    let path = directory.join(file_name);
                    match std::fs::create_dir_all(&directory)
                        .and_then(|()| std::fs::write(&path, png_bytes))
                    {
                        Ok(()) => {
                            if engine.live.occupancy.is_hard_occupied(target.surface) {
                                return;
                            }
                            if let Some(terminal) = engine.runtime.terminals.get_mut(target.surface)
                            {
                                let mut bytes = Vec::new();
                                if *bracketed {
                                    bytes.extend_from_slice(b"\x1b[200~");
                                }
                                bytes.extend_from_slice(path.to_string_lossy().as_bytes());
                                if *bracketed {
                                    bytes.extend_from_slice(b"\x1b[201~");
                                }
                                terminal.send_bytes(&bytes);
                            }
                        }
                        Err(error) => tracing::warn!(%error,"clipboard image save failed"),
                    }
                }
            }
            #[cfg(feature = "gui")]
            Self::AddExplorerFavorite { path, label } => {
                engine
                    .runtime
                    .explorer_favorites
                    .add(path.clone(), label.clone());
                engine.runtime.explorer_favorites.save();
            }
            #[cfg(feature = "gui")]
            Self::TogglePortFavorite {
                address,
                port,
                label,
            } => {
                if engine.runtime.port_favorites.contains(*address, *port) {
                    engine.runtime.port_favorites.remove(*address, *port);
                } else {
                    engine
                        .runtime
                        .port_favorites
                        .add(*address, *port, label.clone());
                }
                engine.runtime.port_favorites.save();
            }
            #[cfg(feature = "gui")]
            Self::DetachSurface { surface, grant } => {
                if engine
                    .live
                    .occupancy
                    .occupancy_of(*surface)
                    .is_some_and(|lock| lock.granted_seq == *grant)
                {
                    engine.release_occupancy(*surface);
                }
            }
            Self::DetachWorkspace {
                workspace,
                holder,
                grant,
            } => {
                if engine
                    .live
                    .occupancy
                    .workspaces_snapshot()
                    .into_iter()
                    .any(|(id, lock)| {
                        id == *workspace && lock.holder == *holder && lock.granted_seq == *grant
                    })
                {
                    engine.force_detach_workspace(*workspace);
                }
            }
            Self::ExplorerCwd { target, folder } => {
                if !target.current(&engine.as_ref()) {
                    return;
                }
                if let Some(explorer) =
                    engine
                        .runtime
                        .surfaces
                        .get_mut(&target.surface)
                        .and_then(|surface| {
                            surface
                                .as_any_mut()
                                .downcast_mut::<crate::model::ExplorerPanel>()
                        })
                {
                    explorer.active_tab_mut().set_cwd(folder.clone());
                    engine.mark_layout_dirty();
                }
            }
            #[cfg(feature = "gui")]
            Self::RemoveExplorerFavorite { path } => {
                engine.runtime.explorer_favorites.remove(path);
                engine.runtime.explorer_favorites.save();
            }
            #[cfg(feature = "gui")]
            Self::RemoteMeshFull { targets } => {
                for target in targets {
                    if target.current(&engine.as_ref()) {
                        engine
                            .remote
                            .pending_mesh_full_resend_forward
                            .insert(target.surface);
                    }
                }
            }
            #[cfg(feature = "gui")]
            Self::LocalMesh {
                target,
                plugin,
                registration,
                bootstrap,
                params,
            } => {
                if !target.current(&engine.as_ref()) {
                    return;
                }
                let Some(kind) = engine
                    .core
                    .find_surface_by_id(target.surface)
                    .map(|surface| surface.kind.as_str())
                else {
                    return;
                };
                let Some(current) = engine.runtime.surface_registry.get_live(kind) else {
                    return;
                };
                if !registration.matches(&current) {
                    return;
                }
                let Some(manager) = plugins else {
                    return;
                };
                if let Some((kind, file, name)) = bootstrap {
                    let Some(binding) = engine
                        .runtime
                        .surfaces
                        .get(&target.surface)
                        .and_then(|surface| {
                            surface
                                .as_any()
                                .downcast_ref::<crate::runtime::egui_mesh_surface::EguiMeshSurface>(
                                )
                        })
                        .map(|surface| surface.retirement_binding.clone())
                    else {
                        return;
                    };
                    manager.send_egui_mesh_surface_create(
                        plugin,
                        target.surface,
                        kind,
                        file.as_deref(),
                        name,
                        &binding,
                    );
                }
                manager.send_surface_set_context(plugin, params);
            }
            #[cfg(feature = "gui")]
            Self::RemoteMesh {
                target,
                context,
                input,
            } => {
                if !target.current(&engine.as_ref()) {
                    return;
                }
                if let Some(context) = context {
                    engine
                        .remote
                        .pending_mesh_context_forward
                        .insert(target.surface, context.clone());
                }
                if let Some(input) = input {
                    engine
                        .remote
                        .pending_mesh_input_forward
                        .entry(target.surface)
                        .and_modify(|pending| {
                            pending.events.extend(input.events.clone());
                            pending.focused = input.focused;
                            pending.modifiers = input.modifiers;
                            pending.time = input.time;
                        })
                        .or_insert_with(|| input.clone());
                }
            }
            #[cfg(feature = "gui")]
            Self::FocusObserved { target } => {
                if target.current(&engine.as_ref()) {
                    engine.clear_attention_local(target.surface);
                    engine.reconcile_soft_occupancy_on_focus(target.surface);
                }
            }
            Self::RecordTyping { target, at } if target.current(&engine.as_ref()) => {
                engine.live.last_key_input.insert(target.surface, *at);
            }
            Self::RecordTyping { .. } => {}
            #[cfg(feature = "gui")]
            Self::ListDirectory {
                request,
                projection,
                target,
            } => {
                if engine.matches_mirror_projection(request.local_ws_id, projection)
                    && target
                        .as_ref()
                        .is_none_or(|target| target.current(&engine.as_ref()))
                {
                    engine.remote.pending_list_dir_forward.push(request.clone());
                }
            }
            Self::DefaultGrid { cols, rows } => {
                engine.runtime.default_cols = *cols;
                engine.runtime.default_rows = *rows;
            }
            Self::Resize { targets } => {
                let targets = targets
                    .iter()
                    .filter(|(target, _, _)| target.current(&engine.as_ref()))
                    .map(|(target, cols, rows)| (target.surface, *cols, *rows))
                    .collect();
                #[cfg(feature = "gui")]
                crate::app::services::AppServices::resize_terminals(engine, targets);
            }
        }
    }
}

/// The popup transfers tunnel ownership once. Cloned presentation intents cannot reuse it.
#[cfg(feature = "gui")]
#[derive(Clone)]
pub(crate) struct AttachRequest(
    std::sync::Arc<std::sync::Mutex<Option<crate::core::GuiAttachUserReq>>>,
);
#[cfg(feature = "gui")]
impl std::fmt::Debug for AttachRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AttachRequest")
    }
}
#[cfg(feature = "gui")]
impl AttachRequest {
    pub(crate) fn new(request: crate::core::GuiAttachUserReq) -> Self {
        Self(std::sync::Arc::new(std::sync::Mutex::new(Some(request))))
    }
    fn take(&self) -> Option<crate::core::GuiAttachUserReq> {
        match self.0.lock() {
            Ok(mut request) => request.take(),
            Err(error) => {
                tracing::warn!(%error,"attach request lock poisoned");
                error.into_inner().take()
            }
        }
    }
}
