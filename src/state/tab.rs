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
            pane.add_tab_with_shell(tab_id, surface_id, cols, rows, sh.shell_ref(), &sh.args_ref(), waker, cwd.as_deref())?;
        }
        self.send_fast_init(surface_id);
        Ok(())
    }

    /// Add a new tab in the focused pane without switching to it, with optional explicit cwd.
    pub fn add_tab_background(&mut self, explicit_cwd: Option<std::path::PathBuf>) -> anyhow::Result<()> {
        let cwd = explicit_cwd.or_else(|| self.resolve_inherit_cwd());
        let tab_id = self.engine.next_ids.next_tab();
        let surface_id = self.engine.next_ids.next_surface();
        let cols = self.engine.default_cols;
        let rows = self.engine.default_rows;
        let sh = crate::engine_state::ShellConfig::from_settings(&self.engine.settings);
        let waker = self.engine.make_waker(surface_id);

        if self.engine.settings.performance.lazy_pty_init {
            if let Some(pane) = self.focused_pane_mut() {
                pane.add_tab_deferred(tab_id, surface_id, sh.shell_ref(), &sh.args_ref(), cols, rows, waker, cwd.as_deref());
            }
        } else {
            if let Some(pane) = self.focused_pane_mut() {
                pane.add_tab_background_with_shell(tab_id, surface_id, cols, rows, sh.shell_ref(), &sh.args_ref(), waker, cwd.as_deref())?;
            }
            self.send_fast_init(surface_id);
        }
        Ok(())
    }

    /// Add a Markdown viewer tab in the focused pane.
    pub fn add_markdown_tab(&mut self, file_path: String) -> anyhow::Result<()> {
        let tab_id = self.engine.next_ids.next_tab();
        let panel_id = self.engine.next_ids.next_surface(); // reuse surface id counter
        if let Some(pane) = self.focused_pane_mut() {
            pane.add_markdown_tab(tab_id, panel_id, file_path);
        }
        Ok(())
    }

    /// Add a file explorer tab in the focused pane.
    pub fn add_explorer_tab(&mut self, root_path: String) -> anyhow::Result<()> {
        let tab_id = self.engine.next_ids.next_tab();
        let panel_id = self.engine.next_ids.next_surface();
        if let Some(pane) = self.focused_pane_mut() {
            pane.add_explorer_tab(tab_id, panel_id, root_path);
        }
        Ok(())
    }

    /// Add an HTML viewer tab in the focused pane.
    pub fn add_html_tab(&mut self, url: String) -> anyhow::Result<()> {
        let tab_id = self.engine.next_ids.next_tab();
        let panel_id = self.engine.next_ids.next_surface();
        if let Some(pane) = self.focused_pane_mut() {
            pane.add_html_tab(tab_id, panel_id, url);
        }
        Ok(())
    }

    /// Add a new tab in the specified pane (by ID, cross-workspace) without switching active tab.
    pub fn add_tab_to_pane(&mut self, pane_id: u32, explicit_cwd: Option<std::path::PathBuf>) -> anyhow::Result<()> {
        let cwd = explicit_cwd.or_else(|| {
            if self.engine.settings.general.inherit_cwd {
                self.find_pane_by_id(pane_id)
                    .and_then(|p| p.active_terminal())
                    .and_then(|t| t.get_cwd())
            } else {
                None
            }
        });
        let tab_id = self.engine.next_ids.next_tab();
        let surface_id = self.engine.next_ids.next_surface();
        let cols = self.engine.default_cols;
        let rows = self.engine.default_rows;
        let sh = crate::engine_state::ShellConfig::from_settings(&self.engine.settings);
        let waker = self.engine.make_waker(surface_id);

        if self.engine.settings.performance.lazy_pty_init {
            if let Some(pane) = self.find_pane_by_id_mut(pane_id) {
                pane.add_tab_deferred(tab_id, surface_id, sh.shell_ref(), &sh.args_ref(), cols, rows, waker, cwd.as_deref());
            }
        } else {
            if let Some(pane) = self.find_pane_by_id_mut(pane_id) {
                pane.add_tab_background_with_shell(tab_id, surface_id, cols, rows, sh.shell_ref(), &sh.args_ref(), waker, cwd.as_deref())?;
            }
            self.send_fast_init(surface_id);
        }
        Ok(())
    }

    /// Add a Markdown viewer tab in the specified pane (by ID, cross-workspace).
    pub fn add_markdown_tab_to_pane(&mut self, pane_id: u32, file_path: String) -> anyhow::Result<()> {
        let tab_id = self.engine.next_ids.next_tab();
        let panel_id = self.engine.next_ids.next_surface();
        if let Some(pane) = self.find_pane_by_id_mut(pane_id) {
            pane.add_markdown_tab(tab_id, panel_id, file_path);
        }
        Ok(())
    }

    /// Add a file explorer tab in the specified pane (by ID, cross-workspace).
    pub fn add_explorer_tab_to_pane(&mut self, pane_id: u32, root_path: String) -> anyhow::Result<()> {
        let tab_id = self.engine.next_ids.next_tab();
        let panel_id = self.engine.next_ids.next_surface();
        if let Some(pane) = self.find_pane_by_id_mut(pane_id) {
            pane.add_explorer_tab(tab_id, panel_id, root_path);
        }
        Ok(())
    }

    /// Add an HTML viewer tab in the specified pane (by ID, cross-workspace).
    pub fn add_html_tab_to_pane(&mut self, pane_id: u32, url: String) -> anyhow::Result<()> {
        let tab_id = self.engine.next_ids.next_tab();
        let panel_id = self.engine.next_ids.next_surface();
        if let Some(pane) = self.find_pane_by_id_mut(pane_id) {
            pane.add_html_tab(tab_id, panel_id, url);
        }
        Ok(())
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
                            tab.panel_mut().for_each_terminal_mut(&mut |sid, _| {
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
                self.unregister_child(sid);
                self.mark_parent_closed(sid);
                crate::surface_meta::SurfaceMetaStore::remove(sid);
            }
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
                self.engine.closed_items.push(crate::model::ClosedItem::Tab(snapshot));
            }
        }
        // Collect surface IDs (mutable borrow)
        let mut surface_ids = Vec::new();
        if let Some(pane) = self.focused_pane_mut() {
            let active = pane.active_tab;
            if let Some(tab) = pane.tabs.get_mut(active) {
                tab.panel_mut().for_each_terminal_mut(&mut |sid, _| {
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
                crate::surface_meta::SurfaceMetaStore::remove(sid);
            }
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
            cols, rows, sh.shell_ref(), &sh.args_ref(), surface_id, waker, cwd.as_deref(),
        ) {
            Ok(t) => t,
            Err(_) => return false,
        };

        let node = crate::model::SurfaceNode {
            id: surface_id,
            terminal,
            deferred_spawn: None,
        };
        let panel = crate::model::Panel::Terminal(node);

        self.replace_panel_for_surface(surface_id, panel, None)
    }

    /// Convert a surface to Markdown type.
    pub fn convert_surface_to_markdown(&mut self, surface_id: u32, file_path: String) -> bool {
        let panel = crate::model::Panel::Markdown(
            crate::model::MarkdownPanel::new(surface_id, file_path.clone()),
        );
        let name = std::path::Path::new(&file_path)
            .file_name()
            .map(|f| f.to_string_lossy().to_string());
        self.replace_panel_for_surface(surface_id, panel, name)
    }

    /// Convert a surface to Explorer type.
    pub fn convert_surface_to_explorer(&mut self, surface_id: u32) -> bool {
        let root = self.resolve_inherit_cwd()
            .map(|p| p.to_string_lossy().to_string())
            .or_else(|| {
                directories::BaseDirs::new()
                    .map(|d| d.home_dir().to_string_lossy().to_string())
            })
            .unwrap_or_else(|| ".".to_string());
        let panel = crate::model::Panel::Explorer(
            crate::model::ExplorerPanel::new(surface_id, root),
        );
        self.replace_panel_for_surface(surface_id, panel, Some("Explorer".to_string()))
    }

    /// Convert a surface to Html type.
    pub fn convert_surface_to_html(&mut self, surface_id: u32, url: String) -> bool {
        let panel = crate::model::Panel::Html(
            crate::model::HtmlPanel::new(surface_id, url),
        );
        self.replace_panel_for_surface(surface_id, panel, Some("HTML".to_string()))
    }

    /// Replace the panel of the tab containing the given surface_id.
    /// Returns true if the replacement succeeded.
    fn replace_panel_for_surface(
        &mut self,
        surface_id: u32,
        new_panel: crate::model::Panel,
        new_name: Option<String>,
    ) -> bool {
        for workspace in &mut self.engine.workspaces {
            for &pid in &workspace.pane_layout().all_pane_ids() {
                if let Some(pane) = workspace.pane_layout_mut().find_pane_mut(pid) {
                    for tab in &mut pane.tabs {
                        if let Some(panel) = tab.panel_mut_if_initialized() {
                            if panel.contains_surface(surface_id) {
                                let old_panel = tab.take_panel();
                                // Drop old panel (PTY cleanup happens automatically)
                                drop(old_panel);
                                tab.put_panel(new_panel);
                                if let Some(name) = new_name {
                                    tab.explicit_name = Some(name);
                                }
                                return true;
                            }
                        }
                    }
                }
            }
        }
        false
    }

    /// Get the current panel type name for the focused surface.
    pub fn focused_panel_type_name(&self) -> Option<&'static str> {
        let pane = self.focused_pane()?;
        let panel = pane.active_panel()?;
        Some(match panel {
            crate::model::Panel::Terminal(_) => "Terminal",
            crate::model::Panel::SurfaceGroup(_) => "SurfaceGroup",
            crate::model::Panel::Markdown(_) => "Markdown",
            crate::model::Panel::Explorer(_) => "Explorer",
            crate::model::Panel::Html(_) => "Html",
        })
    }
}
