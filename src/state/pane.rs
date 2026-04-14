use crate::model::SplitDirection;

use super::AppState;

impl AppState {
    /// Split the focused pane into two (new independent tab bar).
    pub fn split_pane(&mut self, direction: SplitDirection) -> anyhow::Result<()> {
        let cwd = self.resolve_inherit_cwd();
        let new_pane_id = self.engine.next_ids.next_pane();
        let new_tab_id = self.engine.next_ids.next_tab();
        let new_surface_id = self.engine.next_ids.next_surface();
        let cols = self.engine.default_cols;
        let rows = self.engine.default_rows;

        let shell = self.engine.settings.general.shell.clone();
        let shell_ref = if shell.is_empty() { None } else { Some(shell.as_str()) };
        let shell_args_owned = self.engine.settings.general.effective_shell_args();
        let shell_args: Vec<&str> = shell_args_owned.iter().map(|s| s.as_str()).collect();
        let new_pane =
            crate::model::Pane::new_with_shell(new_pane_id, new_tab_id, new_surface_id, cols, rows, shell_ref, &shell_args, self.engine.make_waker(new_surface_id), cwd.as_deref())?;

        let ws = self.active_workspace_mut();
        let target_pane_id = ws.focused_pane;
        ws.pane_layout_mut()
            .split_pane_in_place(target_pane_id, direction, new_pane);
        ws.focused_pane = new_pane_id;
        self.send_fast_init(new_surface_id);
        Ok(())
    }

    /// Split within the current tab (SurfaceGroup). Appears as one tab.
    pub fn split_surface(&mut self, direction: SplitDirection) -> anyhow::Result<()> {
        let cwd = self.resolve_inherit_cwd();
        let new_surface_id = self.engine.next_ids.next_surface();
        let cols = self.engine.default_cols;
        let rows = self.engine.default_rows;
        let shell = self.engine.settings.general.shell.clone();
        let shell_ref = if shell.is_empty() { None } else { Some(shell.as_str()) };
        let shell_args_owned = self.engine.settings.general.effective_shell_args();
        let shell_args: Vec<&str> = shell_args_owned.iter().map(|s| s.as_str()).collect();
        let waker = self.engine.make_waker(new_surface_id);
        if let Some(pane) = self.focused_pane_mut() {
            pane.split_active_surface_with_shell(direction, new_surface_id, cols, rows, shell_ref, &shell_args, waker, cwd.as_deref())?;
        }
        self.send_fast_init(new_surface_id);
        Ok(())
    }

    /// Split a pane group with cross-workspace target support and optional cwd. Does NOT move focus.
    pub fn split_pane_targeted(
        &mut self,
        target_pane_id: Option<u32>,
        direction: SplitDirection,
        explicit_cwd: Option<std::path::PathBuf>,
        surface_type: crate::model::SurfaceType,
    ) -> anyhow::Result<(u32, u32)> {
        let (ws_idx, resolved_pane_id) = match target_pane_id {
            Some(pid) => {
                let ws_idx = self.find_workspace_index_for_pane(pid)
                    .ok_or_else(|| anyhow::anyhow!("pane {} not found", pid))?;
                (ws_idx, pid)
            }
            None => {
                let ws = &self.engine.workspaces[self.active_workspace];
                (self.active_workspace, ws.focused_pane)
            }
        };

        let new_pane_id = self.engine.next_ids.next_pane();
        let new_tab_id = self.engine.next_ids.next_tab();
        let new_surface_id = self.engine.next_ids.next_surface();

        let new_pane = match surface_type {
            crate::model::SurfaceType::Terminal => {
                let cwd = explicit_cwd.or_else(|| {
                    let ws = &self.engine.workspaces[ws_idx];
                    let pane = ws.pane_layout().find_pane(resolved_pane_id)?;
                    let terminal = pane.active_terminal()?;
                    if self.engine.settings.general.inherit_cwd {
                        terminal.get_cwd()
                    } else {
                        None
                    }
                });
                let cols = self.engine.default_cols;
                let rows = self.engine.default_rows;
                let shell = self.engine.settings.general.shell.clone();
                let shell_ref = if shell.is_empty() { None } else { Some(shell.as_str()) };
                let shell_args_owned = self.engine.settings.general.effective_shell_args();
                let shell_args: Vec<&str> = shell_args_owned.iter().map(|s| s.as_str()).collect();
                crate::model::Pane::new_with_shell(
                    new_pane_id, new_tab_id, new_surface_id, cols, rows,
                    shell_ref, &shell_args, self.engine.make_waker(new_surface_id),
                    cwd.as_deref(),
                )?
            }
            crate::model::SurfaceType::Markdown { file } => {
                let panel = crate::model::Panel::Markdown(
                    crate::model::MarkdownPanel::new(new_surface_id, file),
                );
                crate::model::Pane::new_with_panel(new_pane_id, new_tab_id, "Markdown".to_string(), panel)
            }
            crate::model::SurfaceType::Explorer { path } => {
                let root = path.unwrap_or_else(|| {
                    directories::BaseDirs::new()
                        .map(|d| d.home_dir().to_string_lossy().to_string())
                        .unwrap_or_else(|| ".".to_string())
                });
                let panel = crate::model::Panel::Explorer(
                    crate::model::ExplorerPanel::new(new_surface_id, root),
                );
                crate::model::Pane::new_with_panel(new_pane_id, new_tab_id, "Explorer".to_string(), panel)
            }
        };

        let ws = &mut self.engine.workspaces[ws_idx];
        ws.pane_layout_mut()
            .split_pane_in_place(resolved_pane_id, direction, new_pane);

        self.send_fast_init(new_surface_id);
        Ok((new_pane_id, new_surface_id))
    }

    /// Split a surface with cross-workspace target support and optional cwd. Does NOT move focus.
    /// Only terminal type is supported for surface-level splits.
    pub fn split_surface_targeted(
        &mut self,
        target_surface_id: Option<u32>,
        direction: SplitDirection,
        explicit_cwd: Option<std::path::PathBuf>,
    ) -> anyhow::Result<u32> {
        let cwd = explicit_cwd.or_else(|| {
            match target_surface_id {
                Some(sid) => self.resolve_inherit_cwd_from_surface(sid),
                None => self.resolve_inherit_cwd(),
            }
        });

        let new_surface_id = self.engine.next_ids.next_surface();
        let cols = self.engine.default_cols;
        let rows = self.engine.default_rows;
        let shell = self.engine.settings.general.shell.clone();
        let shell_ref = if shell.is_empty() { None } else { Some(shell.as_str()) };
        let shell_args_owned = self.engine.settings.general.effective_shell_args();
        let shell_args: Vec<&str> = shell_args_owned.iter().map(|s| s.as_str()).collect();
        let waker = self.engine.make_waker(new_surface_id);

        match target_surface_id {
            Some(sid) => {
                let (ws_idx, pane_id) = self.find_workspace_index_for_surface(sid)
                    .ok_or_else(|| anyhow::anyhow!("surface {} not found", sid))?;
                let ws = &mut self.engine.workspaces[ws_idx];
                let pane = ws.pane_layout_mut().find_pane_mut(pane_id)
                    .ok_or_else(|| anyhow::anyhow!("pane {} not found", pane_id))?;
                pane.split_surface_by_id_with_shell(
                    sid, direction, new_surface_id, cols, rows,
                    shell_ref, &shell_args, waker, cwd.as_deref(),
                )?;
            }
            None => {
                if let Some(pane) = self.focused_pane_mut() {
                    pane.split_active_surface_with_shell(
                        direction, new_surface_id, cols, rows,
                        shell_ref, &shell_args, waker, cwd.as_deref(),
                    )?;
                }
            }
        }

        self.send_fast_init(new_surface_id);
        Ok(new_surface_id)
    }

    /// Close the focused pane (unsplit). Returns true if a pane was removed.
    pub fn close_active_pane(&mut self) -> bool {
        let ws = self.active_workspace_mut();
        let target_id = ws.focused_pane;

        // Collect all surface IDs in the pane being closed for claude cleanup
        let mut surface_ids = Vec::new();
        if let Some(pane) = ws.pane_layout_mut().find_pane_mut(target_id) {
            for tab in &mut pane.tabs {
                tab.panel_mut().for_each_terminal_mut(&mut |sid, _| {
                    surface_ids.push(sid);
                });
            }
        }

        let removed = ws.pane_layout_mut().close_pane(target_id);
        if removed {
            // Update focus to the first available pane
            if let Some(first) = ws.pane_layout().first_pane() {
                ws.focused_pane = first.id;
            }
            // Claude parent-child cleanup + surface meta cleanup
            for sid in surface_ids {
                self.unregister_child(sid);
                self.mark_parent_closed(sid);
                crate::surface_meta::SurfaceMetaStore::remove(sid);
            }
        }
        removed
    }

    /// Close the focused surface. For SurfaceGroup, closes the focused surface
    /// within the group. For a single Terminal tab, delegates to close_surface_by_id
    /// which handles tab/pane/workspace cascading properly.
    pub fn close_active_surface(&mut self) -> bool {
        let surface_id;
        if let Some(pane) = self.focused_pane_mut() {
            if let Some(panel) = pane.active_panel_mut() {
                match panel {
                    crate::model::Panel::SurfaceGroup(group) => {
                        surface_id = group.focused_surface;
                        if !group.close_surface(surface_id) {
                            // Last surface in group — fall through to close_surface_by_id
                            // which will handle tab/pane/workspace cascading.
                            return self.close_surface_by_id(surface_id);
                        }
                    }
                    crate::model::Panel::Terminal(node) => {
                        surface_id = node.id;
                        return self.close_surface_by_id(surface_id);
                    }
                    _ => return false,
                }
            } else {
                return false;
            }
        } else {
            return false;
        }
        // Claude parent-child cleanup + surface meta cleanup
        self.unregister_child(surface_id);
        self.mark_parent_closed(surface_id);
        crate::surface_meta::SurfaceMetaStore::remove(surface_id);
        true
    }

    /// Close a specific surface by ID. Cascades up the hierarchy:
    /// surface -> tab -> pane -> workspace as needed.
    /// When `save_snapshot` is true, the closed item is saved for user restore (Ctrl+Shift+T).
    /// Agent/IPC closures should pass false to avoid polluting the user's undo stack.
    pub fn close_surface_by_id(&mut self, surface_id: u32) -> bool {
        self.close_surface_by_id_inner(surface_id, true)
    }

    /// Close without saving snapshot (for IPC/agent-initiated closures).
    pub fn close_surface_by_id_no_snapshot(&mut self, surface_id: u32) -> bool {
        self.close_surface_by_id_inner(surface_id, false)
    }

    fn close_surface_by_id_inner(&mut self, surface_id: u32, save_snapshot: bool) -> bool {
        // Find which workspace and pane contain this surface
        let (ws_idx, pane_id) = match self.find_workspace_index_for_surface(surface_id) {
            Some(v) => v,
            None => return false,
        };

        // Find the tab index containing this surface
        let tab_idx;
        let surface_is_sole_in_tab;
        let can_close_surface_in_group;
        {
            let ws = &mut self.engine.workspaces[ws_idx];
            let pane = match ws.pane_layout_mut().find_pane_mut(pane_id) {
                Some(p) => p,
                None => return false,
            };

            // Find which tab has this surface
            let mut found_tab = None;
            for (i, tab) in pane.tabs.iter().enumerate() {
                if tab.panel().contains_surface(surface_id) {
                    found_tab = Some(i);
                    break;
                }
            }
            tab_idx = match found_tab {
                Some(i) => i,
                None => return false,
            };

            // Check if the surface is the only one in this tab's panel
            match pane.tabs[tab_idx].panel() {
                crate::model::Panel::Terminal(node) if node.id == surface_id => {
                    surface_is_sole_in_tab = true;
                    can_close_surface_in_group = false;
                }
                crate::model::Panel::SurfaceGroup(group) => {
                    // Try closing within the group (fails if it's the only surface)
                    surface_is_sole_in_tab = false;
                    can_close_surface_in_group = !matches!(
                        group.layout(),
                        crate::model::SurfaceGroupLayout::Single(_)
                    ) || group.layout().find_terminal(surface_id).is_none();
                }
                // Non-terminal panels (Markdown, Explorer): sole content of the tab
                crate::model::Panel::Markdown(_) | crate::model::Panel::Explorer(_) => {
                    surface_is_sole_in_tab = true;
                    can_close_surface_in_group = false;
                }
                _ => return false,
            }
        }

        // Case 1: Surface is within a SurfaceGroup with multiple surfaces
        if !surface_is_sole_in_tab && can_close_surface_in_group {
            // Capture surface snapshot before closing (user actions only)
            if save_snapshot {
                let ws = &self.engine.workspaces[ws_idx];
                let pane = ws.pane_layout().find_pane(pane_id).unwrap();
                let tab = &pane.tabs[tab_idx];
                if let Some(node) = tab.panel().find_terminal_node(surface_id) {
                    let snapshot = crate::model::closed_item::ClosedSurface::from_surface_node(node);
                    self.engine.closed_items.push(crate::model::ClosedItem::Surface {
                        surface: snapshot,
                        tab_name: tab.display_name().to_string(),
                    });
                }
            }
            let ws = &mut self.engine.workspaces[ws_idx];
            let pane = ws.pane_layout_mut().find_pane_mut(pane_id).unwrap();
            if let crate::model::Panel::SurfaceGroup(group) = pane.tabs[tab_idx].panel_mut() {
                if group.close_surface(surface_id) {
                    self.unregister_child(surface_id);
                    self.mark_parent_closed(surface_id);
                    crate::surface_meta::SurfaceMetaStore::remove(surface_id);
                    return true;
                }
            }
            return false;
        }

        // Case 2: Surface is the sole content of this tab — close the tab
        {
            let ws = &mut self.engine.workspaces[ws_idx];
            let pane = ws.pane_layout_mut().find_pane_mut(pane_id).unwrap();
            if pane.tabs.len() > 1 {
                // Capture tab snapshot before removing (user actions only)
                if save_snapshot {
                    let snapshot = crate::model::closed_item::ClosedTab::from_tab(&pane.tabs[tab_idx]);
                    self.engine.closed_items.push(crate::model::ClosedItem::Tab(snapshot));
                }

                pane.tabs.remove(tab_idx);
                if pane.active_tab >= pane.tabs.len() {
                    pane.active_tab = pane.tabs.len() - 1;
                }
                self.unregister_child(surface_id);
                self.mark_parent_closed(surface_id);
                crate::surface_meta::SurfaceMetaStore::remove(surface_id);
                return true;
            }
        }

        // Case 3: Last tab in pane -- close the pane
        // (pane snapshot is captured as part of workspace in Case 4/5, or inline here)
        {
            let ws = &mut self.engine.workspaces[ws_idx];
            if ws.pane_layout().all_pane_ids().len() > 1 {
                ws.pane_layout_mut().close_pane(pane_id);
                if let Some(first) = ws.pane_layout().first_pane() {
                    ws.focused_pane = first.id;
                }
                self.unregister_child(surface_id);
                self.mark_parent_closed(surface_id);
                crate::surface_meta::SurfaceMetaStore::remove(surface_id);
                return true;
            }
        }

        // Case 4 & 5: Last pane in workspace — close the workspace
        // Capture workspace snapshot before removing (user actions only)
        if save_snapshot {
            let ws = &self.engine.workspaces[ws_idx];
            self.engine.closed_items.push(crate::model::ClosedItem::from_workspace(ws));
        }
        self.engine.workspaces.remove(ws_idx);
        if self.active_workspace >= self.engine.workspaces.len() && !self.engine.workspaces.is_empty() {
            self.active_workspace = self.engine.workspaces.len() - 1;
        }
        self.unregister_child(surface_id);
        self.mark_parent_closed(surface_id);
        crate::surface_meta::SurfaceMetaStore::remove(surface_id);
        true
    }

    /// Split the focused pane and return the new surface ID.
    /// This is like `split_pane` but returns the new surface_id for callers that need it.
    pub fn split_pane_get_surface(&mut self, direction: SplitDirection) -> anyhow::Result<u32> {
        let new_pane_id = self.engine.next_ids.next_pane();
        let new_tab_id = self.engine.next_ids.next_tab();
        let new_surface_id = self.engine.next_ids.next_surface();
        let cols = self.engine.default_cols;
        let rows = self.engine.default_rows;

        let shell = self.engine.settings.general.shell.clone();
        let shell_ref = if shell.is_empty() { None } else { Some(shell.as_str()) };
        let shell_args_owned = self.engine.settings.general.effective_shell_args();
        let shell_args: Vec<&str> = shell_args_owned.iter().map(|s| s.as_str()).collect();
        let new_pane =
            crate::model::Pane::new_with_shell(new_pane_id, new_tab_id, new_surface_id, cols, rows, shell_ref, &shell_args, self.engine.make_waker(new_surface_id), None)?;

        let ws = self.active_workspace_mut();
        let target_pane_id = ws.focused_pane;
        ws.pane_layout_mut()
            .split_pane_in_place(target_pane_id, direction, new_pane);
        ws.focused_pane = new_pane_id;
        self.send_fast_init(new_surface_id);
        Ok(new_surface_id)
    }

    /// Close a specific pane by its ID (across all workspaces).
    /// Returns true if the pane was found and removed.
    pub fn close_pane_by_id(&mut self, pane_id: u32) -> bool {
        let ws_idx = match self.find_workspace_index_for_pane(pane_id) {
            Some(idx) => idx,
            None => return false,
        };

        // Collect surface IDs for cleanup
        let mut surface_ids = Vec::new();
        if let Some(pane) = self.engine.workspaces[ws_idx].pane_layout_mut().find_pane_mut(pane_id) {
            for tab in &mut pane.tabs {
                tab.panel_mut().for_each_terminal_mut(&mut |sid, _| {
                    surface_ids.push(sid);
                });
            }
        }

        let ws = &mut self.engine.workspaces[ws_idx];
        let removed = ws.pane_layout_mut().close_pane(pane_id);
        if removed {
            if ws.focused_pane == pane_id {
                if let Some(first) = ws.pane_layout().first_pane() {
                    ws.focused_pane = first.id;
                }
            }
            for sid in surface_ids {
                self.unregister_child(sid);
                self.mark_parent_closed(sid);
                crate::surface_meta::SurfaceMetaStore::remove(sid);
            }
        }
        removed
    }
}
