use super::AppState;

impl AppState {
    /// Add a new tab in the focused pane.
    pub fn add_tab(&mut self) -> anyhow::Result<()> {
        let cwd = self.resolve_inherit_cwd();
        let tab_id = self.engine.next_ids.next_tab();
        let surface_id = self.engine.next_ids.next_surface();
        let cols = self.engine.default_cols;
        let rows = self.engine.default_rows;
        let shell = self.engine.settings.general.shell.clone();
        let shell_ref = if shell.is_empty() { None } else { Some(shell.as_str()) };
        let shell_args_owned = self.engine.settings.general.effective_shell_args();
        let shell_args: Vec<&str> = shell_args_owned.iter().map(|s| s.as_str()).collect();
        let waker = self.engine.make_waker(surface_id);
        if let Some(pane) = self.focused_pane_mut() {
            pane.add_tab_with_shell(tab_id, surface_id, cols, rows, shell_ref, &shell_args, waker, cwd.as_deref())?;
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
        let shell = self.engine.settings.general.shell.clone();
        let shell_ref = if shell.is_empty() { None } else { Some(shell.as_str()) };
        let shell_args_owned = self.engine.settings.general.effective_shell_args();
        let shell_args: Vec<&str> = shell_args_owned.iter().map(|s| s.as_str()).collect();
        let waker = self.engine.make_waker(surface_id);

        if self.engine.settings.performance.lazy_pty_init {
            if let Some(pane) = self.focused_pane_mut() {
                pane.add_tab_deferred(tab_id, surface_id, shell_ref, &shell_args, cols, rows, waker, cwd.as_deref());
            }
        } else {
            if let Some(pane) = self.focused_pane_mut() {
                pane.add_tab_background_with_shell(tab_id, surface_id, cols, rows, shell_ref, &shell_args, waker, cwd.as_deref())?;
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
        let shell = self.engine.settings.general.shell.clone();
        let shell_ref = if shell.is_empty() { None } else { Some(shell.as_str()) };
        let shell_args_owned = self.engine.settings.general.effective_shell_args();
        let shell_args: Vec<&str> = shell_args_owned.iter().map(|s| s.as_str()).collect();
        let waker = self.engine.make_waker(surface_id);

        if self.engine.settings.performance.lazy_pty_init {
            if let Some(pane) = self.find_pane_by_id_mut(pane_id) {
                pane.add_tab_deferred(tab_id, surface_id, shell_ref, &shell_args, cols, rows, waker, cwd.as_deref());
            }
        } else {
            if let Some(pane) = self.find_pane_by_id_mut(pane_id) {
                pane.add_tab_background_with_shell(tab_id, surface_id, cols, rows, shell_ref, &shell_args, waker, cwd.as_deref())?;
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
        // Collect surface IDs in the active tab before closing
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
}
