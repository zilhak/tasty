use serde_json::Value;

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

        let sh = crate::engine_state::ShellConfig::from_settings(&self.engine.settings);
        let new_pane = crate::model::Pane::new_with_shell(
            new_pane_id,
            new_tab_id,
            new_surface_id,
            cols,
            rows,
            sh.shell_ref(),
            &sh.args_ref(),
            self.engine.make_waker(new_surface_id),
            cwd.as_deref(),
        )?;

        let ws = self.active_workspace_mut();
        let target_pane_id = ws.focused_pane;
        ws.pane_layout_mut()
            .split_pane_in_place(target_pane_id, direction, new_pane);
        ws.focused_pane = new_pane_id;
        self.send_fast_init(new_surface_id);
        self.engine.mark_layout_dirty();
        self.enqueue_host_event(super::PendingHostEvent::PaneSplit {
            original_pane: target_pane_id,
            new_pane: new_pane_id,
            direction,
        });
        Ok(())
    }

    /// Split within the current tab. Appears as one tab.
    /// Only terminal panels support surface-level splitting; others fall back to pane split.
    pub fn split_surface(&mut self, direction: SplitDirection) -> anyhow::Result<()> {
        let target_surface_id = self.focused_surface_id();
        let new_surface_id = self.split_surface_targeted(
            target_surface_id,
            direction,
            None,
            "terminal",
            &Value::Null,
        )?;

        // 단축키(사용자 행위)로 split한 경우 새 surface로 포커스 이동
        let ws = self.active_workspace_mut();
        let focused_pane_id = ws.focused_pane;
        if let Some(pane) = ws.pane_layout_mut().find_pane_mut(focused_pane_id) {
            if let Some(tab) = pane.active_tab_mut() {
                tab.focused_surface = new_surface_id;
            }
        }
        self.engine.mark_layout_dirty();
        Ok(())
    }

    /// Split a pane group with cross-workspace target support and optional cwd. Does NOT move focus.
    ///
    /// `kind` is a SurfaceKindRegistry identifier. `"terminal"`은 호스트 PTY spawn 경로를 사용하고,
    /// 그 외 kind는 `engine.surface_registry`의 create 함수를 호출한다.
    pub fn split_pane_targeted(
        &mut self,
        target_pane_id: Option<u32>,
        direction: SplitDirection,
        explicit_cwd: Option<std::path::PathBuf>,
        kind: &str,
        params: &Value,
    ) -> anyhow::Result<(u32, u32)> {
        let (ws_idx, resolved_pane_id) = match target_pane_id {
            Some(pid) => {
                let ws_idx = self
                    .find_workspace_index_for_pane(pid)
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

        let new_pane = if kind == "terminal" {
            let cwd = explicit_cwd.or_else(|| {
                let ws = &self.engine.workspaces[ws_idx];
                let pane = ws.pane_layout().find_pane(resolved_pane_id)?;
                let tab = pane.tabs.get(pane.active_tab)?;
                let sid = tab.focused_surface_id()?;
                self.resolve_inherit_cwd_from_surface(sid)
            });
            let cols = self.engine.default_cols;
            let rows = self.engine.default_rows;
            let sh = crate::engine_state::ShellConfig::from_settings(&self.engine.settings);
            crate::model::Pane::new_with_shell(
                new_pane_id,
                new_tab_id,
                new_surface_id,
                cols,
                rows,
                sh.shell_ref(),
                &sh.args_ref(),
                self.engine.make_waker(new_surface_id),
                cwd.as_deref(),
            )?
        } else {
            let surface = self.create_surface_via_registry(kind, new_surface_id, params)?;
            let name = default_tab_name_for_kind(kind, params);
            crate::model::Pane::new_with_surface(new_pane_id, new_tab_id, name, surface)
        };

        let ws = &mut self.engine.workspaces[ws_idx];
        ws.pane_layout_mut()
            .split_pane_in_place(resolved_pane_id, direction, new_pane);

        self.send_fast_init(new_surface_id);
        self.engine.mark_layout_dirty();
        self.enqueue_host_event(super::PendingHostEvent::PaneSplit {
            original_pane: resolved_pane_id,
            new_pane: new_pane_id,
            direction,
        });
        Ok((new_pane_id, new_surface_id))
    }

    /// Split a surface with cross-workspace target support and optional cwd. Does NOT move focus.
    /// Supports all surface kinds via SurfaceKindRegistry. `"terminal"`은 PTY spawn 경로를 사용한다.
    pub fn split_surface_targeted(
        &mut self,
        target_surface_id: Option<u32>,
        direction: SplitDirection,
        explicit_cwd: Option<std::path::PathBuf>,
        kind: &str,
        params: &Value,
    ) -> anyhow::Result<u32> {
        let new_surface_id = self.engine.next_ids.next_surface();

        let new_surface: Box<dyn crate::model::Surface> = if kind == "terminal" {
            let cwd = explicit_cwd.or_else(|| match target_surface_id {
                Some(sid) => self.resolve_inherit_cwd_from_surface(sid),
                None => self.resolve_inherit_cwd(),
            });
            let cols = self.engine.default_cols;
            let rows = self.engine.default_rows;
            let sh = crate::engine_state::ShellConfig::from_settings(&self.engine.settings);
            let waker = self.engine.make_waker(new_surface_id);
            let terminal = tasty_terminal::Terminal::new(
                tasty_terminal::TerminalConfig {
                    cols,
                    rows,
                    shell: sh.shell_ref(),
                    args: &sh.args_ref(),
                    surface_id: new_surface_id,
                    working_dir: cwd.as_deref(),
                },
                waker,
            )?;
            Box::new(crate::model::TerminalSurface {
                id: new_surface_id,
                terminal,
                deferred_spawn: None,
            })
        } else {
            self.create_surface_via_registry(kind, new_surface_id, params)?
        };

        match target_surface_id {
            Some(sid) => {
                let (ws_idx, pane_id) = self
                    .find_workspace_index_for_surface(sid)
                    .ok_or_else(|| anyhow::anyhow!("surface {} not found", sid))?;
                let ws = &mut self.engine.workspaces[ws_idx];
                let pane = ws
                    .pane_layout_mut()
                    .find_pane_mut(pane_id)
                    .ok_or_else(|| anyhow::anyhow!("pane {} not found", pane_id))?;
                pane.split_surface_by_id_with_surface(sid, direction, new_surface)?;
            }
            None => {
                anyhow::bail!("target_surface_id is required for surface-level splits");
            }
        }

        self.send_fast_init(new_surface_id);
        self.engine.mark_layout_dirty();
        Ok(new_surface_id)
    }

    /// Close the focused pane (unsplit). Returns true if a pane was removed.
    pub fn close_active_pane(&mut self) -> bool {
        let ws = self.active_workspace_mut();
        let target_id = ws.focused_pane;

        // Collect all surface IDs in the pane being closed for cleanup.
        let mut surface_ids = Vec::new();
        if let Some(pane) = ws.pane_layout_mut().find_pane_mut(target_id) {
            for tab in &mut pane.tabs {
                tab.for_each_terminal_mut(&mut |sid, _| {
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
            // Surface meta + per-surface view cleanup.
            for sid in surface_ids {
                self.cleanup_surface(sid);
            }
            self.engine.mark_layout_dirty();
        }
        removed
    }

    /// Close the focused surface. For split tabs, closes the focused surface
    /// within the tab. For single-surface tabs, delegates to close_surface_by_id
    /// which handles tab/pane/workspace cascading.
    pub fn close_active_surface(&mut self) -> bool {
        let surface_id;
        if let Some(pane) = self.focused_pane_mut() {
            let tab = match pane.active_tab_mut() {
                Some(t) => t,
                None => return false,
            };
            surface_id = tab.focused_surface;
            if tab.is_split() {
                if !tab.close_surface(surface_id) {
                    return self.close_surface_by_id(surface_id);
                }
            } else {
                return self.close_surface_by_id(surface_id);
            }
        } else {
            return false;
        }
        // Surface meta + per-surface view cleanup.
        self.cleanup_surface(surface_id);
        self.engine.mark_layout_dirty();
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
                if tab.contains_surface(surface_id) {
                    found_tab = Some(i);
                    break;
                }
            }
            tab_idx = match found_tab {
                Some(i) => i,
                None => return false,
            };

            // Check if the surface is the only one in this tab
            let tab = &pane.tabs[tab_idx];
            if tab.is_split() {
                // Split tab: try closing within the layout (fails if it's the only surface)
                surface_is_sole_in_tab = false;
                can_close_surface_in_group =
                    !matches!(tab.layout(), crate::model::SurfaceLayout::Leaf(_));
            } else if tab.contains_surface(surface_id) {
                // Single-surface tab: sole content
                surface_is_sole_in_tab = true;
                can_close_surface_in_group = false;
            } else {
                return false;
            }
        }

        // Case 1: Surface is within a split tab with multiple surfaces
        if !surface_is_sole_in_tab && can_close_surface_in_group {
            // Capture surface snapshot before closing (user actions only)
            if save_snapshot {
                let ws = &self.engine.workspaces[ws_idx];
                let pane = ws.pane_layout().find_pane(pane_id).unwrap();
                let tab = &pane.tabs[tab_idx];
                if let Some(node) = tab.find_terminal_surface(surface_id) {
                    let snapshot =
                        crate::model::closed_item::ClosedSurface::from_surface_node(node);
                    self.engine
                        .push_closed_item(crate::model::ClosedItem::Surface {
                            surface: snapshot,
                            tab_name: tab.display_name().to_string(),
                        });
                }
            }
            let ws = &mut self.engine.workspaces[ws_idx];
            let pane = ws.pane_layout_mut().find_pane_mut(pane_id).unwrap();
            let tab = &mut pane.tabs[tab_idx];
            if tab.close_surface(surface_id) {
                crate::surface_meta::SurfaceMetaStore::remove(surface_id);
                self.engine.mark_layout_dirty();
                return true;
            }
            return false;
        }

        // Case 2: Surface is the sole content of this tab — close the tab
        {
            // Capture tab snapshot before removing (user actions only).
            // Must be done in a separate scope to avoid borrow conflicts.
            if save_snapshot {
                let ws = &self.engine.workspaces[ws_idx];
                let pane = ws.pane_layout().find_pane(pane_id).unwrap();
                if pane.tabs.len() > 1 {
                    let snapshot_opt = {
                        let mut snap_fn = crate::surface_registry::snapshot_fn_for(
                            &self.engine.surface_registry,
                        );
                        crate::model::closed_item::ClosedTab::from_tab(
                            &pane.tabs[tab_idx],
                            &mut snap_fn,
                        )
                    };
                    if let Some(snapshot) = snapshot_opt {
                        self.engine
                            .push_closed_item(crate::model::ClosedItem::Tab(snapshot));
                    }
                }
            }
            let ws = &mut self.engine.workspaces[ws_idx];
            let pane = ws.pane_layout_mut().find_pane_mut(pane_id).unwrap();
            if pane.tabs.len() > 1 {
                pane.tabs.remove(tab_idx);
                if pane.active_tab >= pane.tabs.len() {
                    pane.active_tab = pane.tabs.len() - 1;
                }
                crate::surface_meta::SurfaceMetaStore::remove(surface_id);
                self.engine.mark_layout_dirty();
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
                crate::surface_meta::SurfaceMetaStore::remove(surface_id);
                self.engine.mark_layout_dirty();
                return true;
            }
        }

        // Case 4 & 5: Last pane in workspace — close the workspace
        // Capture workspace snapshot before removing (user actions only)
        if save_snapshot {
            let item = {
                let mut snap_fn = crate::surface_registry::snapshot_fn_for(
                    &self.engine.surface_registry,
                );
                let ws = &self.engine.workspaces[ws_idx];
                crate::model::ClosedItem::from_workspace(ws, &mut snap_fn)
            };
            self.engine.push_closed_item(item);
        }
        self.engine.workspaces.remove(ws_idx);
        if self.active_workspace >= self.engine.workspaces.len()
            && !self.engine.workspaces.is_empty()
        {
            self.active_workspace = self.engine.workspaces.len() - 1;
        }
        self.cleanup_surface(surface_id);
        self.engine.mark_layout_dirty();
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

        let sh = crate::engine_state::ShellConfig::from_settings(&self.engine.settings);
        let new_pane = crate::model::Pane::new_with_shell(
            new_pane_id,
            new_tab_id,
            new_surface_id,
            cols,
            rows,
            sh.shell_ref(),
            &sh.args_ref(),
            self.engine.make_waker(new_surface_id),
            None,
        )?;

        let ws = self.active_workspace_mut();
        let target_pane_id = ws.focused_pane;
        ws.pane_layout_mut()
            .split_pane_in_place(target_pane_id, direction, new_pane);
        ws.focused_pane = new_pane_id;
        self.send_fast_init(new_surface_id);
        self.engine.mark_layout_dirty();
        self.enqueue_host_event(super::PendingHostEvent::PaneSplit {
            original_pane: target_pane_id,
            new_pane: new_pane_id,
            direction,
        });
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
        if let Some(pane) = self.engine.workspaces[ws_idx]
            .pane_layout_mut()
            .find_pane_mut(pane_id)
        {
            for tab in &mut pane.tabs {
                tab.for_each_terminal_mut(&mut |sid, _| {
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
                self.cleanup_surface(sid);
            }
            self.engine.mark_layout_dirty();
        }
        removed
    }

    /// SurfaceKindRegistry를 통해 새 surface 인스턴스를 만든다.
    /// `"terminal"`은 호출자가 PTY spawn 경로로 분기 처리해야 하므로 여기서는 처리하지 않는다.
    pub(crate) fn create_surface_via_registry(
        &self,
        kind: &str,
        surface_id: u32,
        params: &Value,
    ) -> anyhow::Result<Box<dyn crate::model::Surface>> {
        let def = self
            .engine
            .surface_registry
            .get(kind)
            .ok_or_else(|| anyhow::anyhow!("unknown surface kind: {}", kind))?;
        (def.create)(surface_id, params)
    }
}

/// kind+params로부터 합리적인 탭 표시명을 도출한다.
/// 경로/URL이 있으면 마지막 segment, 없으면 kind에 대응되는 정적 이름을 사용한다.
pub(crate) fn default_tab_name_for_kind(kind: &str, params: &Value) -> String {
    fn basename_or(path: &str, fallback: &str) -> String {
        path.split(['/', '\\'])
            .filter(|s| !s.is_empty())
            .last()
            .unwrap_or(fallback)
            .to_string()
    }
    match kind {
        "markdown" => params
            .get("file")
            .and_then(|v| v.as_str())
            .map(|p| basename_or(p, "Markdown"))
            .unwrap_or_else(|| "Markdown".to_string()),
        "explorer" => params
            .get("path")
            .and_then(|v| v.as_str())
            .map(|p| basename_or(p, "Explorer"))
            .unwrap_or_else(|| "Explorer".to_string()),
        "html" => "HTML".to_string(),
        "image" => params
            .get("file")
            .and_then(|v| v.as_str())
            .map(|p| basename_or(p, "Image"))
            .unwrap_or_else(|| "Image".to_string()),
        "empty" => "Empty".to_string(),
        "terminal" => "terminal".to_string(),
        other => other.to_string(),
    }
}
