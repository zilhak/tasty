//! Non-journal execution requests carry targets observed by a View, never a mutable Engine borrow.
use crate::runtime::engine_access::EngineMut;
use crate::runtime::surface_binding::SurfaceBinding;

impl SurfaceBinding {
    #[cfg(feature = "gui")]
    pub(crate) fn take_mouse_capture_hint(
        &self,
        engine: &mut EngineMut<'_>,
        foreground_generation: u64,
    ) -> Option<u32> {
        if !self.current(&engine.as_ref())
            || engine.foreground_generation(self.surface_id()) != foreground_generation
            || !engine.runtime.settings.general.mouse_capture_hint
            || engine
                .foreground_name(self.surface_id())
                .is_some_and(|name| {
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
            .get_mut(self.surface_id())?
            .take_mouse_capture_hint()
            .then_some(self.surface_id())
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
    Html(crate::runtime::html_script::HtmlAction),
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
    #[cfg(feature = "gui")]
    FocusObserved {
        target: SurfaceBinding,
        /// 창이 OS 포커스를 가졌는지. attention은 사용자가 실제로 볼 때만 지운다.
        window_focused: bool,
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
    #[cfg(feature = "gui")]
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
            Self::Explorer { .. } => self.apply_explorer(engine),
            #[cfg(feature = "gui")]
            Self::DagSelection { .. } => self.apply_dag_selection(engine),
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
            Self::PasteImage { .. } => self.apply_paste_image(engine),
            #[cfg(feature = "gui")]
            Self::AddExplorerFavorite { path, label } => {
                engine.runtime.change_explorer_favorites(|favorites| {
                    favorites.add(path.clone(), label.clone())
                });
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
            Self::ExplorerCwd { .. } => self.apply_explorer_cwd(engine),
            #[cfg(feature = "gui")]
            Self::RemoveExplorerFavorite { path } => {
                engine
                    .runtime
                    .change_explorer_favorites(|favorites| favorites.remove(path));
            }
            #[cfg(feature = "gui")]
            Self::RemoteMeshFull { targets } => {
                for target in targets {
                    if target.current(&engine.as_ref()) {
                        engine
                            .remote
                            .pending_mesh_full_resend_forward
                            .insert(target.surface_id());
                    }
                }
            }
            #[cfg(feature = "gui")]
            Self::LocalMesh { .. } => self.apply_local_mesh(engine, plugins),
            #[cfg(feature = "gui")]
            Self::RemoteMesh { .. } => self.apply_remote_mesh(engine),
            #[cfg(feature = "gui")]
            Self::FocusObserved {
                target,
                window_focused,
            } => {
                if target.current(&engine.as_ref()) {
                    if *window_focused {
                        engine.clear_attention_local(target.surface_id());
                    }
                    engine.reconcile_soft_occupancy_on_focus(target.surface_id());
                }
            }
            Self::RecordTyping { target, at } if target.current(&engine.as_ref()) => {
                engine.live.last_key_input.insert(target.surface_id(), *at);
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
            #[cfg(feature = "gui")]
            Self::Resize { targets } => {
                let targets = targets
                    .iter()
                    .filter(|(target, _, _)| target.current(&engine.as_ref()))
                    .map(|(target, cols, rows)| (target.surface_id(), *cols, *rows))
                    .collect();
                #[cfg(feature = "gui")]
                crate::app::services::AppServices::resize_terminals(engine, targets);
            }
        }
    }

    #[cfg(feature = "gui")]
    fn apply_paste_image(&self, engine: &mut EngineMut<'_>) {
        let Self::PasteImage {
            target,
            view,
            bracketed,
            file_name,
            png_bytes,
        } = self
        else {
            unreachable!("variant-specific action dispatch")
        };
        if !target.current(&engine.as_ref()) {
            return;
        }
        if let Some(workspace) = target.mirror_workspace() {
            engine
                .remote
                .pending_image_uploads
                .push(crate::core::PendingImageUpload {
                    origin_view: view.clone(),
                    mirror_ws_id: workspace,
                    surface_id: target.surface_id(),
                    bracketed: *bracketed,
                    file_name: file_name.clone(),
                    png_bytes: png_bytes.clone(),
                });
        } else {
            let directory = match tempfile::Builder::new()
                .prefix("tasty-clipboard-")
                .tempdir()
            {
                Ok(directory) => directory,
                Err(error) => {
                    tracing::warn!(%error, "clipboard image directory creation failed");
                    return;
                }
            };
            let path = directory.path().join(file_name);
            match std::fs::write(&path, png_bytes) {
                Ok(()) => {
                    // The receiving shell reads this file asynchronously, after this action.
                    drop(directory.keep());
                    send_saved_image_path(engine, target.surface_id(), *bracketed, &path);
                }
                Err(error) => tracing::warn!(%error,"clipboard image save failed"),
            }
        }
    }
    #[cfg(feature = "gui")]
    fn apply_local_mesh(
        &self,
        engine: &mut EngineMut<'_>,
        plugins: Option<&crate::plugin::PluginManager>,
    ) {
        let Self::LocalMesh {
            target,
            plugin,
            registration,
            bootstrap,
            params,
        } = self
        else {
            unreachable!("variant-specific action dispatch")
        };
        if !target.current(&engine.as_ref()) {
            return;
        }
        let Some(kind) = engine
            .core
            .find_surface_by_id(target.surface_id())
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
                .get(&target.surface_id())
                .and_then(|surface| {
                    surface
                        .as_any()
                        .downcast_ref::<crate::runtime::egui_mesh_surface::EguiMeshSurface>()
                })
                .map(|surface| surface.retirement_binding.clone())
            else {
                return;
            };
            manager.send_egui_mesh_surface_create(
                plugin,
                target.surface_id(),
                kind,
                file.as_deref(),
                name,
                &binding,
            );
        }
        manager.send_surface_set_context(plugin, params);
    }
    #[cfg(feature = "gui")]
    fn apply_remote_mesh(&self, engine: &mut EngineMut<'_>) {
        let Self::RemoteMesh {
            target,
            context,
            input,
        } = self
        else {
            unreachable!("variant-specific action dispatch")
        };
        if !target.current(&engine.as_ref()) {
            return;
        }
        if let Some(context) = context {
            engine
                .remote
                .pending_mesh_context_forward
                .insert(target.surface_id(), context.clone());
        }
        if let Some(input) = input {
            engine
                .remote
                .pending_mesh_input_forward
                .entry(target.surface_id())
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
    fn apply_explorer(&self, engine: &mut EngineMut<'_>) {
        let Self::Explorer { target, action } = self else {
            unreachable!("variant-specific action dispatch")
        };
        if target.current(&engine.as_ref())
            && let Some(panel) = engine
                .runtime
                .surfaces
                .get_mut(&target.surface_id())
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
    fn apply_dag_selection(&self, engine: &mut EngineMut<'_>) {
        let Self::DagSelection {
            target,
            dag_id,
            direction,
        } = self
        else {
            unreachable!("variant-specific action dispatch")
        };
        if target.current(&engine.as_ref())
            && let Some(dag) = engine
                .runtime
                .surfaces
                .get_mut(&target.surface_id())
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

    fn apply_explorer_cwd(&self, engine: &mut EngineMut<'_>) {
        let Self::ExplorerCwd { target, folder } = self else {
            unreachable!("variant-specific action dispatch")
        };
        if !target.current(&engine.as_ref()) {
            return;
        }
        if let Some(explorer) = engine
            .runtime
            .surfaces
            .get_mut(&target.surface_id())
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
}

#[cfg(feature = "gui")]
fn send_saved_image_path(
    engine: &mut EngineMut<'_>,
    surface: u32,
    bracketed: bool,
    path: &std::path::Path,
) {
    if engine.live.occupancy.is_hard_occupied(surface) {
        return;
    }
    if let Some(terminal) = engine.runtime.terminals.get_mut(surface) {
        let mut bytes = Vec::new();
        if bracketed {
            bytes.extend_from_slice(b"\x1b[200~");
        }
        bytes.extend_from_slice(path.to_string_lossy().as_bytes());
        if bracketed {
            bytes.extend_from_slice(b"\x1b[201~");
        }
        terminal.send_bytes(&bytes);
    }
}

#[cfg(all(test, feature = "gui"))]
mod tests {
    use super::EngineAction;
    use crate::core::state::attention::AttentionKind;
    use crate::runtime::surface_binding::SurfaceBinding;

    /// 창이 OS 포커스를 잃은 동안 그린 포커스 surface는 attention을 지우지 않는다.
    #[test]
    fn focus_observation_clears_attention_only_while_the_window_is_focused() {
        let mut session = crate::state::tests::test_state().1;
        let sid = session
            .borrow_mut()
            .workspace_at(0)
            .expect("workspace index is valid")
            .all_surface_ids()[0];
        let target = SurfaceBinding::capture(&session.read(), sid).expect("binding");
        session
            .borrow_mut()
            .raise_attention(sid, AttentionKind::NeedsInput);

        EngineAction::FocusObserved {
            target: target.clone(),
            window_focused: false,
        }
        .apply(&mut session.borrow_mut(), None);
        assert_eq!(
            session.borrow_mut().attention_kind(sid),
            Some(AttentionKind::NeedsInput)
        );

        EngineAction::FocusObserved {
            target,
            window_focused: true,
        }
        .apply(&mut session.borrow_mut(), None);
        assert_eq!(session.borrow_mut().attention_kind(sid), None);
    }

    fn engine_with(
        registries: &crate::runtime::registries::RuntimeRegistries,
    ) -> crate::runtime::engine_session::EngineSession {
        let memory: std::sync::Arc<std::sync::Mutex<dyn tasty_memory::MemoryStorage>> =
            std::sync::Arc::new(std::sync::Mutex::new(
                tasty_memory::MemoryStore::open_in_memory().expect("in-memory store"),
            ));
        crate::runtime::engine_session::EngineSession::for_journal(
            crate::runtime::engine_session::EngineSessionSpec {
                cols: 80,
                rows: 24,
                waker: std::sync::Arc::new(|| {}),
                shared_ids: None,
                layout_slot: None,
                memory,
                runner_registry: std::sync::Arc::new(tasty_task_runtime::RunnerRegistry::new()),
            },
            crate::settings::Settings::default(),
            registries.clone(),
        )
        .expect("engine")
    }

    /// 윈도우마다 engine 이 있어도 즐겨찾기 추가가 서로를 지우지 않고, 다른 윈도우의 사본도 맞춰진다.
    #[test]
    fn favorites_added_in_two_windows_both_survive_a_restart() {
        let _home = crate::test_support::IsolatedHome::new();
        let registries = crate::runtime::registries::RuntimeRegistries::new(None);
        let mut first = engine_with(&registries);
        let mut second = engine_with(&registries);
        let add = |session: &mut crate::runtime::engine_session::EngineSession, name: &str| {
            EngineAction::AddExplorerFavorite {
                path: crate::test_support::abs_path(name),
                label: String::new(),
            }
            .apply(&mut session.borrow_mut(), None);
        };
        add(&mut first, "w/alpha");
        add(&mut second, "w/beta");
        let labels = |favorites: &crate::core::explorer_favorites::ExplorerFavorites| {
            favorites
                .items
                .iter()
                .map(|f| f.label.clone())
                .collect::<Vec<_>>()
        };

        // 재시작: 새 프로세스 원본은 파일에서 읽는다.
        let restarted = crate::runtime::registries::RuntimeRegistries::new(None);
        assert_eq!(
            labels(&restarted.explorer_favorites.copy().1),
            ["alpha", "beta"]
        );

        // 첫 윈도우의 사본은 다음 그리기 전에 다른 윈도우의 추가를 받는다.
        assert_eq!(labels(&first.runtime.explorer_favorites), ["alpha"]);
        assert!(first.runtime.sync_explorer_favorites());
        assert_eq!(labels(&first.runtime.explorer_favorites), ["alpha", "beta"]);
        assert!(!first.runtime.sync_explorer_favorites(), "already current");

        EngineAction::RemoveExplorerFavorite {
            path: crate::test_support::abs_path("w/alpha"),
        }
        .apply(&mut second.borrow_mut(), None);
        assert_eq!(
            labels(&crate::core::explorer_favorites::ExplorerFavorites::load()),
            ["beta"]
        );
    }
}
