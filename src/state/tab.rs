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

    /// Add a Markdown viewer tab in the focused pane.
    pub fn add_markdown_tab(&mut self, file_path: String) -> anyhow::Result<()> {
        let tab_id = self.engine.next_ids.next_tab();
        let surface_id = self.engine.next_ids.next_surface();
        let name = file_path
            .split(['/', '\\'])
            .last()
            .unwrap_or("Markdown")
            .to_string();
        let surface: Box<dyn crate::model::Surface> =
            Box::new(crate::model::MarkdownPanel::new(surface_id, file_path));
        if let Some(pane) = self.focused_pane_mut() {
            pane.add_surface_tab(tab_id, name, surface);
            self.engine.mark_layout_dirty();
        }
        Ok(())
    }

    /// Add a file explorer tab in the focused pane.
    pub fn add_explorer_tab(&mut self, root_path: String) -> anyhow::Result<()> {
        let tab_id = self.engine.next_ids.next_tab();
        let surface_id = self.engine.next_ids.next_surface();
        let name = root_path
            .split(['/', '\\'])
            .last()
            .unwrap_or("Explorer")
            .to_string();
        let surface: Box<dyn crate::model::Surface> =
            Box::new(crate::model::ExplorerPanel::new(surface_id, root_path));
        if let Some(pane) = self.focused_pane_mut() {
            pane.add_surface_tab(tab_id, name, surface);
            self.engine.mark_layout_dirty();
        }
        Ok(())
    }

    /// Add an HTML viewer tab in the focused pane.
    pub fn add_html_tab(&mut self, url: String) -> anyhow::Result<()> {
        let tab_id = self.engine.next_ids.next_tab();
        let surface_id = self.engine.next_ids.next_surface();
        let surface: Box<dyn crate::model::Surface> =
            Box::new(crate::model::HtmlPanel::new(surface_id, url));
        if let Some(pane) = self.focused_pane_mut() {
            pane.add_surface_tab(tab_id, "HTML".to_string(), surface);
            self.engine.mark_layout_dirty();
        }
        Ok(())
    }

    /// Add a clipboard viewer tab in the focused pane.
    pub fn add_clipboard_viewer_tab(&mut self) -> anyhow::Result<()> {
        let tab_id = self.engine.next_ids.next_tab();
        let surface_id = self.engine.next_ids.next_surface();
        let surface: Box<dyn crate::model::Surface> =
            Box::new(crate::model::ClipboardViewerPanel::new(surface_id));
        let name = crate::i18n::t("clipboard_viewer.tab_title").to_string();
        if let Some(pane) = self.focused_pane_mut() {
            pane.add_surface_tab(tab_id, name, surface);
            self.engine.mark_layout_dirty();
        }
        Ok(())
    }

    /// Add a clipboard viewer tab to the specified pane (cross-workspace).
    pub fn add_clipboard_viewer_tab_to_pane(&mut self, pane_id: u32) -> anyhow::Result<()> {
        let tab_id = self.engine.next_ids.next_tab();
        let surface_id = self.engine.next_ids.next_surface();
        let surface: Box<dyn crate::model::Surface> =
            Box::new(crate::model::ClipboardViewerPanel::new(surface_id));
        let name = crate::i18n::t("clipboard_viewer.tab_title").to_string();
        if let Some(pane) = self.find_pane_by_id_mut(pane_id) {
            pane.add_surface_tab(tab_id, name, surface);
            self.engine.mark_layout_dirty();
        }
        Ok(())
    }

    /// Add an empty placeholder tab in the focused pane. Returns (tab_id, surface_id).
    pub fn add_empty_tab(&mut self) -> Option<(u32, u32)> {
        let tab_id = self.engine.next_ids.next_tab();
        let surface_id = self.engine.next_ids.next_surface();
        let surface: Box<dyn crate::model::Surface> =
            Box::new(crate::model::EmptySurface::new(surface_id));
        if let Some(pane) = self.focused_pane_mut() {
            pane.add_surface_tab(tab_id, "Empty".to_string(), surface);
            self.engine.mark_layout_dirty();
            Some((tab_id, surface_id))
        } else {
            None
        }
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
        // Find the tab and collect surface IDs for cleanup
        let mut surface_ids = Vec::new();
        let mut found_pane_id = None;
        for workspace in &mut self.engine.workspaces {
            for &pid in &workspace.pane_layout().all_pane_ids() {
                if let Some(pane) = workspace.pane_layout_mut().find_pane_mut(pid) {
                    if let Some(tab_idx) = pane.tabs.iter().position(|t| t.id == tab_id) {
                        if let Some(tab) = pane.tabs.get_mut(tab_idx) {
                            tab.for_each_terminal_mut(&mut |sid, _| {
                                surface_ids.push(sid);
                            });
                        }
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
            for sid in surface_ids {
                self.cleanup_surface(sid);
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
        // Capture tab snapshot before closing (immutable borrow)
        if let Some(pane) = self.focused_pane() {
            let active = pane.active_tab;
            if let Some(tab) = pane.tabs.get(active) {
                let snapshot = crate::model::closed_item::ClosedTab::from_tab(tab);
                self.engine
                    .closed_items
                    .push(crate::model::ClosedItem::Tab(snapshot));
            }
        }
        // Collect surface IDs (mutable borrow)
        let mut surface_ids = Vec::new();
        if let Some(pane) = self.focused_pane_mut() {
            let active = pane.active_tab;
            if let Some(tab) = pane.tabs.get_mut(active) {
                tab.for_each_terminal_mut(&mut |sid, _| {
                    surface_ids.push(sid);
                });
            }
        }
        let closed = if let Some(pane) = self.focused_pane_mut() {
            pane.close_active_tab()
        } else {
            false
        };
        if closed {
            for sid in surface_ids {
                self.cleanup_surface(sid);
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

        let terminal = match tasty_terminal::Terminal::new_with_shell_args_cwd(
            cols,
            rows,
            sh.shell_ref(),
            &sh.args_ref(),
            surface_id,
            waker,
            cwd.as_deref(),
        ) {
            Ok(t) => t,
            Err(_) => return false,
        };

        let node = crate::model::TerminalSurface {
            id: surface_id,
            terminal,
            deferred_spawn: None,
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

    /// Convert a surface to Explorer type.
    pub fn convert_surface_to_explorer(&mut self, surface_id: u32) -> bool {
        let root = self
            .resolve_inherit_cwd()
            .map(|p| p.to_string_lossy().to_string())
            .or_else(|| {
                directories::BaseDirs::new().map(|d| d.home_dir().to_string_lossy().to_string())
            })
            .unwrap_or_else(|| ".".to_string());
        let surface: Box<dyn crate::model::Surface> =
            Box::new(crate::model::ExplorerPanel::new(surface_id, root));
        self.replace_surface_for_id(surface_id, surface, Some(Some("Explorer".to_string())))
    }

    /// Add an image viewer tab in the focused pane.
    pub fn add_image_tab(&mut self, file_path: String) -> anyhow::Result<()> {
        let tab_id = self.engine.next_ids.next_tab();
        let surface_id = self.engine.next_ids.next_surface();
        let name = file_path
            .split(['/', '\\'])
            .last()
            .unwrap_or("Image")
            .to_string();
        let surface: Box<dyn crate::model::Surface> =
            Box::new(crate::model::ImagePanel::new(surface_id, file_path));
        if let Some(pane) = self.focused_pane_mut() {
            pane.add_surface_tab(tab_id, name, surface);
            self.engine.mark_layout_dirty();
        }
        Ok(())
    }

    /// Convert a surface to Image type (blank canvas).
    pub fn convert_surface_to_image(&mut self, surface_id: u32) -> bool {
        let surface: Box<dyn crate::model::Surface> =
            Box::new(crate::model::ImagePanel::new_blank(surface_id));
        self.replace_surface_for_id(surface_id, surface, Some(Some("Image".to_string())))
    }

    /// Convert a surface to Html type.
    pub fn convert_surface_to_html(&mut self, surface_id: u32, url: String) -> bool {
        let surface: Box<dyn crate::model::Surface> =
            Box::new(crate::model::HtmlPanel::new(surface_id, url));
        self.replace_surface_for_id(surface_id, surface, Some(Some("HTML".to_string())))
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
