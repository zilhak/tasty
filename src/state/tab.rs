use serde_json::{Value, json};

use super::AppState;

impl AppState {
    /// Add a new tab in the focused pane.
    pub fn add_tab(&mut self) -> anyhow::Result<()> {
        let cwd = self.resolve_inherit_cwd();
        let tab_id = self.engine.next_ids.next_tab();
        let surface_id = self.engine.next_ids.next_surface();
        let cols = self.engine.default_cols;
        let rows = self.engine.default_rows;
        let sh = crate::engine_state::ShellConfig::from_settings(&self.engine.settings);
        let waker = self.engine.make_waker(surface_id);
        if let Some(pane) = self.focused_pane_mut() {
            pane.add_tab_with_shell(
                tab_id,
                surface_id,
                cols,
                rows,
                sh.shell_ref(),
                &sh.args_ref(),
                waker,
                cwd.as_deref(),
            )?;
        }
        self.send_fast_init(surface_id);
        self.engine.mark_layout_dirty();
        Ok(())
    }

    /// Add a new tab in the focused pane without switching to it, with optional explicit cwd.
    pub fn add_tab_background(
        &mut self,
        explicit_cwd: Option<std::path::PathBuf>,
    ) -> anyhow::Result<()> {
        let cwd = explicit_cwd.or_else(|| self.resolve_inherit_cwd());
        let tab_id = self.engine.next_ids.next_tab();
        let surface_id = self.engine.next_ids.next_surface();
        let cols = self.engine.default_cols;
        let rows = self.engine.default_rows;
        let sh = crate::engine_state::ShellConfig::from_settings(&self.engine.settings);
        let waker = self.engine.make_waker(surface_id);

        if self.engine.settings.performance.lazy_pty_init {
            if let Some(pane) = self.focused_pane_mut() {
                pane.add_tab_deferred(
                    tab_id,
                    surface_id,
                    sh.shell_ref(),
                    &sh.args_ref(),
                    cols,
                    rows,
                    waker,
                    cwd.as_deref(),
                );
            }
        } else {
            if let Some(pane) = self.focused_pane_mut() {
                pane.add_tab_background_with_shell(
                    tab_id,
                    surface_id,
                    cols,
                    rows,
                    sh.shell_ref(),
                    &sh.args_ref(),
                    waker,
                    cwd.as_deref(),
                )?;
            }
            self.send_fast_init(surface_id);
        }
        self.engine.mark_layout_dirty();
        Ok(())
    }

    /// Generic kind+params 기반 탭 추가. SurfaceKindRegistry를 통해 surface를 만들고
    /// 포커스된 pane에 부착한다. Returns (tab_id, surface_id) on success.
    pub fn add_kind_tab(&mut self, kind: &str, params: &Value) -> anyhow::Result<(u32, u32)> {
        let tab_id = self.engine.next_ids.next_tab();
        let surface_id = self.engine.next_ids.next_surface();
        let surface = self.create_surface_via_registry(kind, surface_id, params)?;
        let name = super::pane::default_tab_name_for_kind(kind, params);
        if let Some(pane) = self.focused_pane_mut() {
            pane.add_surface_tab(tab_id, name, surface);
            self.engine.mark_layout_dirty();
            Ok((tab_id, surface_id))
        } else {
            anyhow::bail!("no focused pane to add tab to")
        }
    }

    /// kind+params 탭을 특정 pane에 추가 (cross-workspace).
    pub fn add_kind_tab_to_pane(
        &mut self,
        pane_id: u32,
        kind: &str,
        params: &Value,
    ) -> anyhow::Result<(u32, u32)> {
        let tab_id = self.engine.next_ids.next_tab();
        let surface_id = self.engine.next_ids.next_surface();
        let surface = self.create_surface_via_registry(kind, surface_id, params)?;
        let name = super::pane::default_tab_name_for_kind(kind, params);
        if let Some(pane) = self.find_pane_by_id_mut(pane_id) {
            pane.add_surface_tab(tab_id, name, surface);
            self.engine.mark_layout_dirty();
            Ok((tab_id, surface_id))
        } else {
            anyhow::bail!("pane {} not found", pane_id)
        }
    }

    /// Add a Markdown viewer tab in the focused pane.
    pub fn add_markdown_tab(&mut self, file_path: String) -> anyhow::Result<()> {
        self.add_kind_tab("markdown", &json!({"file": file_path}))
            .map(|_| ())
    }

    /// Add an empty placeholder tab in the focused pane. Returns (tab_id, surface_id).
    pub fn add_empty_tab(&mut self) -> Option<(u32, u32)> {
        self.add_kind_tab("empty", &Value::Null).ok()
    }

    /// Add a new tab in the specified pane (by ID, cross-workspace) without switching active tab.
    pub fn add_tab_to_pane(
        &mut self,
        pane_id: u32,
        explicit_cwd: Option<std::path::PathBuf>,
    ) -> anyhow::Result<()> {
        let cwd = explicit_cwd.or_else(|| {
            let pane = self.find_pane_by_id(pane_id)?;
            let tab = pane.tabs.get(pane.active_tab)?;
            let sid = tab.focused_surface_id()?;
            self.resolve_inherit_cwd_from_surface(sid)
        });
        let tab_id = self.engine.next_ids.next_tab();
        let surface_id = self.engine.next_ids.next_surface();
        let cols = self.engine.default_cols;
        let rows = self.engine.default_rows;
        let sh = crate::engine_state::ShellConfig::from_settings(&self.engine.settings);
        let waker = self.engine.make_waker(surface_id);

        if self.engine.settings.performance.lazy_pty_init {
            if let Some(pane) = self.find_pane_by_id_mut(pane_id) {
                pane.add_tab_deferred(
                    tab_id,
                    surface_id,
                    sh.shell_ref(),
                    &sh.args_ref(),
                    cols,
                    rows,
                    waker,
                    cwd.as_deref(),
                );
            }
        } else {
            if let Some(pane) = self.find_pane_by_id_mut(pane_id) {
                pane.add_tab_background_with_shell(
                    tab_id,
                    surface_id,
                    cols,
                    rows,
                    sh.shell_ref(),
                    &sh.args_ref(),
                    waker,
                    cwd.as_deref(),
                )?;
            }
            self.send_fast_init(surface_id);
        }
        self.engine.mark_layout_dirty();
        Ok(())
    }

    /// Add a tab with a Surface trait object in the specified pane (by ID, cross-workspace).
    pub fn add_surface_tab_to_pane(
        &mut self,
        pane_id: u32,
        name: String,
        surface: Box<dyn crate::model::Surface>,
    ) {
        let tab_id = self.engine.next_ids.next_tab();
        if let Some(pane) = self.find_pane_by_id_mut(pane_id) {
            pane.add_surface_tab(tab_id, name, surface);
            self.engine.mark_layout_dirty();
        }
    }

    /// Close a specific tab by its TabId (cross-workspace). Returns true if closed.
    pub fn close_tab_by_tab_id(&mut self, tab_id: u32) -> bool {
        // Find the tab and collect (surface_id, persist_id) for cleanup
        let mut targets: Vec<(u32, Option<String>)> = Vec::new();
        let mut found_pane_id = None;
        for workspace in &self.engine.workspaces {
            for &pid in &workspace.pane_layout().all_pane_ids() {
                if let Some(pane) = workspace.pane_layout().find_pane(pid) {
                    if let Some(tab) = pane.tabs.iter().find(|t| t.id == tab_id) {
                        super::AppState::collect_close_targets(tab, &mut targets);
                        found_pane_id = Some(pid);
                        break;
                    }
                }
            }
            if found_pane_id.is_some() {
                break;
            }
        }

        let pane_id = match found_pane_id {
            Some(pid) => pid,
            None => return false,
        };

        let closed = if let Some(pane) = self.find_pane_by_id_mut(pane_id) {
            pane.close_tab_by_id(tab_id)
        } else {
            false
        };
        if closed {
            for (sid, pid) in targets {
                self.cleanup_surface(sid, pid);
            }
            self.engine.mark_layout_dirty();
        }
        closed
    }

    /// Next tab in the focused pane.
    pub fn next_tab_in_pane(&mut self) {
        if let Some(pane) = self.focused_pane_mut() {
            pane.next_tab();
        }
    }

    /// Previous tab in the focused pane.
    pub fn prev_tab_in_pane(&mut self) {
        if let Some(pane) = self.focused_pane_mut() {
            pane.prev_tab();
        }
    }

    /// Go to tab by index (0-based) in the focused pane.
    pub fn goto_tab_in_pane(&mut self, index: usize) -> bool {
        if let Some(pane) = self.focused_pane_mut() {
            pane.goto_tab(index)
        } else {
            false
        }
    }

    /// Close the active tab in the focused pane. Returns true if a tab was closed.
    pub fn close_active_tab(&mut self) -> bool {
        // Capture tab snapshot + collect persist_ids (immutable borrow).
        let mut targets: Vec<(u32, Option<String>)> = Vec::new();
        let snapshot_opt = if let Some(pane) = self.focused_pane() {
            let active = pane.active_tab;
            if let Some(tab) = pane.tabs.get(active) {
                super::AppState::collect_close_targets(tab, &mut targets);
                let mut snap_fn =
                    crate::surface_registry::snapshot_fn_for(&self.engine.surface_registry);
                crate::model::closed_item::ClosedTab::from_tab(tab, &mut snap_fn)
            } else {
                None
            }
        } else {
            None
        };
        if let Some(snapshot) = snapshot_opt {
            self.engine
                .push_closed_item(crate::model::ClosedItem::Tab(snapshot));
        }
        let closed = if let Some(pane) = self.focused_pane_mut() {
            pane.close_active_tab()
        } else {
            false
        };
        if closed {
            for (sid, pid) in targets {
                self.cleanup_surface(sid, pid);
            }
            self.engine.mark_layout_dirty();
        }
        closed
    }

    /// Convert a surface to Terminal type. Creates a new PTY.
    pub fn convert_surface_to_terminal(&mut self, surface_id: u32) -> bool {
        let cwd = self.resolve_inherit_cwd();
        let cols = self.engine.default_cols;
        let rows = self.engine.default_rows;
        let sh = crate::engine_state::ShellConfig::from_settings(&self.engine.settings);
        let waker = self.engine.make_waker(surface_id);

        let terminal = match tasty_terminal::Terminal::new(
            tasty_terminal::TerminalConfig {
                cols,
                rows,
                shell: sh.shell_ref(),
                args: &sh.args_ref(),
                surface_id,
                working_dir: cwd.as_deref(),
                initial_input: None,
            },
            waker,
        ) {
            Ok(t) => t,
            Err(_) => return false,
        };

        let node = crate::model::TerminalSurface {
            id: surface_id,
            terminal,
            deferred_spawn: None,
            scrollback_persist_id: None,
        };
        let surface: Box<dyn crate::model::Surface> = Box::new(node);

        // Clear explicit_name when converting back to Terminal (auto-derived from CWD).
        let replaced = self.replace_surface_for_id(surface_id, surface, Some(None));
        if replaced {
            self.send_fast_init(surface_id);
        }
        replaced
    }

    /// Convert a surface to Markdown type.
    pub fn convert_surface_to_markdown(&mut self, surface_id: u32, file_path: String) -> bool {
        let surface: Box<dyn crate::model::Surface> = Box::new(crate::model::MarkdownPanel::new(
            surface_id,
            file_path.clone(),
        ));
        let name = std::path::Path::new(&file_path)
            .file_name()
            .map(|f| f.to_string_lossy().to_string());
        self.replace_surface_for_id(surface_id, surface, Some(name))
    }

    /// Add an image viewer tab in the focused pane.
    pub fn add_image_tab(&mut self, file_path: String) -> anyhow::Result<()> {
        self.add_kind_tab("image", &json!({"file": file_path}))
            .map(|_| ())
    }

    /// Convert a surface to Image type (blank canvas).
    pub fn convert_surface_to_image(&mut self, surface_id: u32) -> bool {
        let surface: Box<dyn crate::model::Surface> =
            Box::new(crate::model::ImagePanel::new_blank(surface_id));
        self.replace_surface_for_id(surface_id, surface, Some(Some("Image".to_string())))
    }

    /// Convert a surface to an arbitrary registered kind via the surface registry.
    /// Plugin이 제공하는 kind(예: "explorer") 변환에 사용된다. 빌트인 중 특수
    /// 파라미터(파일 경로, URL 등)가 필요한 kind는 전용 메서드 (`convert_surface_to_markdown` 등)
    /// 를 사용해야 한다.
    pub fn convert_surface_to_kind(&mut self, surface_id: u32, kind: &str, params: &Value) -> bool {
        let new_surface = match self.create_surface_via_registry(kind, surface_id, params) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("convert_surface_to_kind('{}') failed: {}", kind, e);
                return false;
            }
        };
        // 변환 시 explicit_name은 클리어. tab 표시명은 surface 자체의 display_name을 따른다.
        self.replace_surface_for_id(surface_id, new_surface, Some(None))
    }

    /// Replace a specific surface by ID. If the surface is inside a split tab,
    /// only that individual leaf is replaced — other surfaces in the tab are unaffected.
    /// If it's the sole surface in a tab, the tab's surface is replaced entirely.
    /// `new_name` updates `explicit_name` only for standalone (non-split) surfaces.
    /// Pass `Some(None)` to clear explicit_name (e.g., when converting back to Terminal).
    fn replace_surface_for_id(
        &mut self,
        surface_id: u32,
        new_surface: Box<dyn crate::model::Surface>,
        new_name: Option<Option<String>>,
    ) -> bool {
        // First, find the location (workspace index, pane id, tab index).
        let mut location: Option<(usize, u32, usize)> = None;
        'outer: for (ws_idx, workspace) in self.engine.workspaces.iter().enumerate() {
            for &pid in &workspace.pane_layout().all_pane_ids() {
                if let Some(pane) = workspace.pane_layout().find_pane(pid) {
                    for (tab_idx, tab) in pane.tabs.iter().enumerate() {
                        if tab.contains_surface(surface_id) {
                            location = Some((ws_idx, pid, tab_idx));
                            break 'outer;
                        }
                    }
                }
            }
        }

        let (ws_idx, pane_id, tab_idx) = match location {
            Some(loc) => loc,
            None => return false,
        };

        let ws = &mut self.engine.workspaces[ws_idx];
        let pane = match ws.pane_layout_mut().find_pane_mut(pane_id) {
            Some(p) => p,
            None => return false,
        };
        let tab = &mut pane.tabs[tab_idx];

        // Case 1: Tab has split layout — replace just the leaf.
        if tab.is_split() {
            let replaced = tab.layout_mut().replace_surface(surface_id, new_surface);
            if replaced {
                self.engine.mark_layout_dirty();
            }
            return replaced;
            // Don't change tab name for individual surface replacement within a split.
        }
        // Case 2: Tab's sole surface — replace the whole tab surface.
        tab.put_surface(new_surface);
        if let Some(name_opt) = new_name {
            tab.explicit_name = name_opt;
        }
        self.engine.mark_layout_dirty();
        true
    }

    /// Get the current surface type name for the focused surface.
    pub fn focused_panel_type_name(&self) -> Option<&'static str> {
        let pane = self.focused_pane()?;
        let tab = pane.tabs.get(pane.active_tab)?;
        Some(tab.surface().type_name())
    }
}
