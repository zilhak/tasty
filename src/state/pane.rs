use serde_json::Value;

use crate::engine_state::EngineState;
use crate::model::SplitDirection;

use super::AppState;

impl AppState {
    /// Split the focused pane into two (new independent tab bar).
    pub fn split_pane(
        &mut self,
        engine: &mut EngineState,
        direction: SplitDirection,
    ) -> anyhow::Result<()> {
        let cwd = self.resolve_inherit_cwd(engine);
        let new_pane_id = engine.next_ids.next_pane();
        let new_tab_id = engine.next_ids.next_tab();
        let new_surface_id = engine.next_ids.next_surface();
        let cols = engine.default_cols;
        let rows = engine.default_rows;

        let sh = crate::engine_state::ShellConfig::from_settings(&engine.settings);
        let new_pane = crate::model::Pane::new_with_shell(
            new_pane_id,
            new_tab_id,
            new_surface_id,
            crate::model::ShellSpawnOpts {
                cols: cols,
                rows: rows,
                shell: sh.shell_ref(),
                shell_args: &sh.args_ref(),
                waker: engine.make_waker(new_surface_id),
                working_dir: cwd.as_deref(),
            },
        )?;

        let ws = self.active_workspace_mut(engine);
        let target_pane_id = ws.focused_pane;
        ws.pane_layout_mut()
            .split_pane_in_place(target_pane_id, direction, new_pane);
        ws.focused_pane = new_pane_id;
        engine.send_fast_init(new_surface_id);
        engine.mark_layout_dirty();
        self.enqueue_host_event(super::PendingHostEvent::PaneSplit {
            original_pane: target_pane_id,
            new_pane: new_pane_id,
            direction,
        });
        Ok(())
    }

    /// Split within the current tab. Appears as one tab.
    /// Only terminal panels support surface-level splitting; others fall back to pane split.
    pub fn split_surface(
        &mut self,
        engine: &mut EngineState,
        direction: SplitDirection,
    ) -> anyhow::Result<()> {
        let target_surface_id = self.focused_surface_id(engine);
        let new_surface_id = self.split_surface_targeted(
            engine,
            target_surface_id,
            direction,
            None,
            "terminal",
            &Value::Null,
        )?;

        // 단축키(사용자 행위)로 split한 경우 새 surface로 포커스 이동
        let ws = self.active_workspace_mut(engine);
        let focused_pane_id = ws.focused_pane;
        if let Some(pane) = ws.pane_layout_mut().find_pane_mut(focused_pane_id) {
            if let Some(tab) = pane.active_tab_mut() {
                tab.focused_surface = new_surface_id;
            }
        }
        engine.mark_layout_dirty();
        Ok(())
    }

    /// Split a pane group with cross-workspace target support and optional cwd. Does NOT move focus.
    ///
    /// `kind` is a SurfaceKindRegistry identifier. `"terminal"`은 호스트 PTY spawn 경로를 사용하고,
    /// 그 외 kind는 `engine.surface_registry`의 create 함수를 호출한다.
    pub fn split_pane_targeted(
        &mut self,
        engine: &mut EngineState,
        target_pane_id: Option<u32>,
        direction: SplitDirection,
        explicit_cwd: Option<std::path::PathBuf>,
        kind: &str,
        params: &Value,
    ) -> anyhow::Result<(u32, u32)> {
        let (ws_idx, resolved_pane_id) = match target_pane_id {
            Some(pid) => {
                let ws_idx = engine
                    .find_workspace_index_for_pane(pid)
                    .ok_or_else(|| anyhow::anyhow!("pane {} not found", pid))?;
                (ws_idx, pid)
            }
            None => {
                let ws = &engine.workspaces[self.active_workspace];
                (self.active_workspace, ws.focused_pane)
            }
        };

        let new_pane_id = engine.next_ids.next_pane();
        let new_tab_id = engine.next_ids.next_tab();
        let new_surface_id = engine.next_ids.next_surface();

        let new_pane = if kind == "terminal" {
            let cwd = explicit_cwd.or_else(|| {
                let ws = &engine.workspaces[ws_idx];
                let pane = ws.pane_layout().find_pane(resolved_pane_id)?;
                let tab = pane.tabs.get(pane.active_tab)?;
                let sid = tab.focused_surface_id()?;
                self.resolve_inherit_cwd_from_surface(engine, sid)
            });
            let cols = engine.default_cols;
            let rows = engine.default_rows;
            let sh = crate::engine_state::ShellConfig::from_settings(&engine.settings);
            crate::model::Pane::new_with_shell(
                new_pane_id,
                new_tab_id,
                new_surface_id,
                crate::model::ShellSpawnOpts {
                    cols: cols,
                    rows: rows,
                    shell: sh.shell_ref(),
                    shell_args: &sh.args_ref(),
                    waker: engine.make_waker(new_surface_id),
                    working_dir: cwd.as_deref(),
                },
            )?
        } else {
            let surface = self.create_surface_via_registry(engine, kind, new_surface_id, params)?;
            let name = default_tab_name_for_kind(kind, params);
            crate::model::Pane::new_with_surface(new_pane_id, new_tab_id, name, surface)
        };

        let ws = &mut engine.workspaces[ws_idx];
        ws.pane_layout_mut()
            .split_pane_in_place(resolved_pane_id, direction, new_pane);

        engine.send_fast_init(new_surface_id);
        engine.mark_layout_dirty();
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
        engine: &mut EngineState,
        target_surface_id: Option<u32>,
        direction: SplitDirection,
        explicit_cwd: Option<std::path::PathBuf>,
        kind: &str,
        params: &Value,
    ) -> anyhow::Result<u32> {
        let new_surface_id = engine.next_ids.next_surface();

        let new_surface: Box<dyn crate::model::Surface> = if kind == "terminal" {
            let cwd = explicit_cwd.or_else(|| match target_surface_id {
                Some(sid) => self.resolve_inherit_cwd_from_surface(engine, sid),
                None => self.resolve_inherit_cwd(engine),
            });
            let cols = engine.default_cols;
            let rows = engine.default_rows;
            let sh = crate::engine_state::ShellConfig::from_settings(&engine.settings);
            let waker = engine.make_waker(new_surface_id);
            let terminal = tasty_terminal::Terminal::new(
                tasty_terminal::TerminalConfig {
                    cols,
                    rows,
                    shell: sh.shell_ref(),
                    args: &sh.args_ref(),
                    surface_id: new_surface_id,
                    working_dir: cwd.as_deref(),
                    initial_input: None,
                },
                waker,
            )?;
            Box::new(crate::model::TerminalSurface {
                id: new_surface_id,
                terminal,
                deferred_spawn: None,
                scrollback_persist_id: None,
            })
        } else {
            self.create_surface_via_registry(engine, kind, new_surface_id, params)?
        };

        match target_surface_id {
            Some(sid) => {
                let (ws_idx, pane_id) = engine
                    .find_workspace_index_for_surface(sid)
                    .ok_or_else(|| anyhow::anyhow!("surface {} not found", sid))?;
                let ws = &mut engine.workspaces[ws_idx];
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

        engine.send_fast_init(new_surface_id);
        engine.mark_layout_dirty();
        Ok(new_surface_id)
    }

    /// Close the focused pane (unsplit). Returns true if a pane was removed.
    pub fn close_active_pane(&mut self, engine: &mut EngineState) -> bool {
        let ws = self.active_workspace_mut(engine);
        let target_id = ws.focused_pane;

        // Collect all (surface_id, persist_id) in the pane being closed for cleanup.
        let mut targets: Vec<(u32, Option<String>)> = Vec::new();
        if let Some(pane) = ws.pane_layout().find_pane(target_id) {
            for tab in &pane.tabs {
                Self::collect_close_targets(tab, &mut targets);
            }
        }

        let ws = self.active_workspace_mut(engine);
        let removed = ws.pane_layout_mut().close_pane(target_id);
        if removed {
            // Update focus to the first available pane
            if let Some(first) = ws.pane_layout().first_pane() {
                ws.focused_pane = first.id;
            }
            for (sid, pid) in targets {
                self.cleanup_surface(engine, sid, pid);
            }
            engine.mark_layout_dirty();
        }
        removed
    }

    /// Close the focused surface. For split tabs, closes the focused surface
    /// within the tab. For single-surface tabs, delegates to close_surface_by_id
    /// which handles tab/pane/workspace cascading.
    pub fn close_active_surface(&mut self, engine: &mut EngineState) -> bool {
        let surface_id;
        let persist_id;
        if let Some(pane) = self.focused_pane(engine) {
            let tab = match pane.tabs.get(pane.active_tab) {
                Some(t) => t,
                None => return false,
            };
            surface_id = tab.focused_surface;
            persist_id = tab
                .find_terminal_surface(surface_id)
                .and_then(|ts| ts.scrollback_persist_id.clone());
        } else {
            return false;
        }
        let split_handled;
        if let Some(pane) = self.focused_pane_mut(engine) {
            let tab = match pane.active_tab_mut() {
                Some(t) => t,
                None => return false,
            };
            if tab.is_split() {
                if !tab.close_surface(surface_id) {
                    return self.close_surface_by_id(engine, surface_id);
                }
                split_handled = true;
            } else {
                return self.close_surface_by_id(engine, surface_id);
            }
        } else {
            return false;
        }
        if split_handled {
            self.cleanup_surface(engine, surface_id, persist_id);
            engine.mark_layout_dirty();
        }
        true
    }

    /// Close a specific surface by ID. Cascades up the hierarchy:
    /// surface -> tab -> pane -> workspace as needed.
    /// When `save_snapshot` is true, the closed item is saved for user restore (Ctrl+Shift+T).
    /// Agent/IPC closures should pass false to avoid polluting the user's undo stack.
    pub fn close_surface_by_id(&mut self, engine: &mut EngineState, surface_id: u32) -> bool {
        self.close_surface_by_id_inner(engine, surface_id, true)
    }

    /// Close without saving snapshot (for IPC/agent-initiated closures).
    ///
    /// Agent 가 마지막 workspace 까지 닫아 windows 상태가 비어 버리면, 다음
    /// redraw 가 `active_workspace()` 를 호출하다 패닉한다. 사용자의 window 를
    /// 에이전트가 끄는 부작용도 피해야 하므로 (CLAUDE.md "사용자 행동과 에이전트
    /// 행동의 분리"), cascade 결과 workspaces 가 비면 즉시 새 empty workspace
    /// 를 만들어 invariant 를 유지한다.
    pub fn close_surface_by_id_no_snapshot(
        &mut self,
        engine: &mut EngineState,
        surface_id: u32,
    ) -> bool {
        let closed = self.close_surface_by_id_inner(engine, surface_id, false);
        if closed && engine.workspaces.is_empty() {
            if let Err(e) = self.add_workspace(engine) {
                tracing::warn!(
                    "close_surface_by_id_no_snapshot: auto-recreate workspace failed: {e}"
                );
            }
        }
        closed
    }

    fn close_surface_by_id_inner(
        &mut self,
        engine: &mut EngineState,
        surface_id: u32,
        save_snapshot: bool,
    ) -> bool {
        // Find which workspace and pane contain this surface
        let (ws_idx, pane_id) = match engine.find_workspace_index_for_surface(surface_id) {
            Some(v) => v,
            None => return false,
        };

        // Find the tab index containing this surface
        let tab_idx;
        let surface_is_sole_in_tab;
        let can_close_surface_in_group;
        {
            let ws = &mut engine.workspaces[ws_idx];
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
                let ws = &engine.workspaces[ws_idx];
                let pane = ws.pane_layout().find_pane(pane_id).unwrap();
                let tab = &pane.tabs[tab_idx];
                if let Some(node) = tab.find_terminal_surface(surface_id) {
                    let snapshot =
                        crate::model::closed_item::ClosedSurface::from_surface_node(node);
                    engine.push_closed_item(crate::model::ClosedItem::Surface {
                        surface: snapshot,
                        tab_name: tab.display_name().to_string(),
                    });
                }
            }
            // close 이전에 leaf surface 의 persist_id 를 추출해 둔다.
            let persist_id = {
                let ws = &engine.workspaces[ws_idx];
                let pane = ws.pane_layout().find_pane(pane_id).unwrap();
                let tab = &pane.tabs[tab_idx];
                tab.find_terminal_surface(surface_id)
                    .and_then(|ts| ts.scrollback_persist_id.clone())
            };
            let ws = &mut engine.workspaces[ws_idx];
            let pane = ws.pane_layout_mut().find_pane_mut(pane_id).unwrap();
            let tab = &mut pane.tabs[tab_idx];
            if tab.close_surface(surface_id) {
                self.cleanup_surface(engine, surface_id, persist_id);
                engine.mark_layout_dirty();
                return true;
            }
            return false;
        }

        // Case 2: Surface is the sole content of this tab — close the tab
        {
            // Capture tab snapshot before removing (user actions only).
            // Must be done in a separate scope to avoid borrow conflicts.
            if save_snapshot {
                let ws = &engine.workspaces[ws_idx];
                let pane = ws.pane_layout().find_pane(pane_id).unwrap();
                if pane.tabs.len() > 1 {
                    let snapshot_opt = {
                        let mut snap_fn = crate::engine::surface_registry::snapshot_fn_for(
                            &engine.surface_registry,
                        );
                        crate::model::closed_item::ClosedTab::from_tab(
                            &pane.tabs[tab_idx],
                            &mut snap_fn,
                        )
                    };
                    if let Some(snapshot) = snapshot_opt {
                        engine.push_closed_item(crate::model::ClosedItem::Tab(snapshot));
                    }
                }
            }
            // tab 의 모든 leaf surface 의 persist_id 수집 후 close.
            let mut targets: Vec<(u32, Option<String>)> = Vec::new();
            {
                let ws = &engine.workspaces[ws_idx];
                let pane = ws.pane_layout().find_pane(pane_id).unwrap();
                if pane.tabs.len() > 1 {
                    Self::collect_close_targets(&pane.tabs[tab_idx], &mut targets);
                }
            }
            let ws = &mut engine.workspaces[ws_idx];
            let pane = ws.pane_layout_mut().find_pane_mut(pane_id).unwrap();
            if pane.tabs.len() > 1 {
                pane.tabs.remove(tab_idx);
                if pane.active_tab >= pane.tabs.len() {
                    pane.active_tab = pane.tabs.len() - 1;
                }
                for (sid, pid) in targets {
                    self.cleanup_surface(engine, sid, pid);
                }
                engine.mark_layout_dirty();
                return true;
            }
        }

        // Case 3: Last tab in pane -- close the pane
        // (pane snapshot is captured as part of workspace in Case 4/5, or inline here)
        {
            // pane 내 모든 tab 의 leaf surface persist_id 수집.
            let mut targets: Vec<(u32, Option<String>)> = Vec::new();
            {
                let ws = &engine.workspaces[ws_idx];
                if ws.pane_layout().all_pane_ids().len() > 1 {
                    if let Some(pane) = ws.pane_layout().find_pane(pane_id) {
                        for tab in &pane.tabs {
                            Self::collect_close_targets(tab, &mut targets);
                        }
                    }
                }
            }
            let ws = &mut engine.workspaces[ws_idx];
            if ws.pane_layout().all_pane_ids().len() > 1 {
                ws.pane_layout_mut().close_pane(pane_id);
                if let Some(first) = ws.pane_layout().first_pane() {
                    ws.focused_pane = first.id;
                }
                for (sid, pid) in targets {
                    self.cleanup_surface(engine, sid, pid);
                }
                engine.mark_layout_dirty();
                return true;
            }
        }

        // Case 4 & 5: Last pane in workspace — close the workspace
        // Capture workspace snapshot before removing (user actions only)
        if save_snapshot {
            let item = {
                let mut snap_fn =
                    crate::engine::surface_registry::snapshot_fn_for(&engine.surface_registry);
                let ws = &engine.workspaces[ws_idx];
                crate::model::ClosedItem::from_workspace(ws, &mut snap_fn)
            };
            engine.push_closed_item(item);
        }
        // Workspace 전체의 모든 leaf surface persist_id 수집 (제거 전).
        let mut targets: Vec<(u32, Option<String>)> = Vec::new();
        {
            let ws = &engine.workspaces[ws_idx];
            for pid in ws.pane_layout().all_pane_ids() {
                if let Some(pane) = ws.pane_layout().find_pane(pid) {
                    for tab in &pane.tabs {
                        Self::collect_close_targets(tab, &mut targets);
                    }
                }
            }
        }
        let workspace_id = engine.workspaces[ws_idx].id;
        engine.workspaces.remove(ws_idx);
        if self.active_workspace >= engine.workspaces.len() && !engine.workspaces.is_empty() {
            self.active_workspace = engine.workspaces.len() - 1;
        }
        // Workspace scope 의 memory entry 정리 (마지막 surface 가 닫혀 workspace 도 사라지는 경로).
        let ws_scope = tasty_memory::Scope::Workspace(workspace_id);
        tasty_memory::with_store(|s| match s.purge_scope(&ws_scope) {
            Ok(stats) if stats.regular + stats.secret > 0 => tracing::debug!(
                workspace_id,
                regular = stats.regular,
                secret = stats.secret,
                "memory: purged closed-workspace scope",
            ),
            Ok(_) => {}
            Err(e) => tracing::warn!(workspace_id, "memory: purge_scope failed: {e}"),
        });
        for (sid, pid) in targets {
            self.cleanup_surface(engine, sid, pid);
        }
        engine.mark_layout_dirty();
        true
    }

    /// Split the focused pane and return the new surface ID.
    /// This is like `split_pane` but returns the new surface_id for callers that need it.
    pub fn split_pane_get_surface(
        &mut self,
        engine: &mut EngineState,
        direction: SplitDirection,
    ) -> anyhow::Result<u32> {
        let new_pane_id = engine.next_ids.next_pane();
        let new_tab_id = engine.next_ids.next_tab();
        let new_surface_id = engine.next_ids.next_surface();
        let cols = engine.default_cols;
        let rows = engine.default_rows;

        let sh = crate::engine_state::ShellConfig::from_settings(&engine.settings);
        let new_pane = crate::model::Pane::new_with_shell(
            new_pane_id,
            new_tab_id,
            new_surface_id,
            crate::model::ShellSpawnOpts {
                cols: cols,
                rows: rows,
                shell: sh.shell_ref(),
                shell_args: &sh.args_ref(),
                waker: engine.make_waker(new_surface_id),
                working_dir: None,
            },
        )?;

        let ws = self.active_workspace_mut(engine);
        let target_pane_id = ws.focused_pane;
        ws.pane_layout_mut()
            .split_pane_in_place(target_pane_id, direction, new_pane);
        ws.focused_pane = new_pane_id;
        engine.send_fast_init(new_surface_id);
        engine.mark_layout_dirty();
        self.enqueue_host_event(super::PendingHostEvent::PaneSplit {
            original_pane: target_pane_id,
            new_pane: new_pane_id,
            direction,
        });
        Ok(new_surface_id)
    }

    /// Close a specific pane by its ID (across all workspaces).
    /// Returns true if the pane was found and removed.
    pub fn close_pane_by_id(&mut self, engine: &mut EngineState, pane_id: u32) -> bool {
        let ws_idx = match engine.find_workspace_index_for_pane(pane_id) {
            Some(idx) => idx,
            None => return false,
        };

        // Collect (surface_id, persist_id) for cleanup
        let mut targets: Vec<(u32, Option<String>)> = Vec::new();
        if let Some(pane) = engine.workspaces[ws_idx].pane_layout().find_pane(pane_id) {
            for tab in &pane.tabs {
                Self::collect_close_targets(tab, &mut targets);
            }
        }

        let ws = &mut engine.workspaces[ws_idx];
        let removed = ws.pane_layout_mut().close_pane(pane_id);
        if removed {
            if ws.focused_pane == pane_id {
                if let Some(first) = ws.pane_layout().first_pane() {
                    ws.focused_pane = first.id;
                }
            }
            for (sid, pid) in targets {
                self.cleanup_surface(engine, sid, pid);
            }
            engine.mark_layout_dirty();
        }
        removed
    }

    /// SurfaceKindRegistry를 통해 새 surface 인스턴스를 만든다.
    /// `"terminal"`은 호출자가 PTY spawn 경로로 분기 처리해야 하므로 여기서는 처리하지 않는다.
    pub(crate) fn create_surface_via_registry(
        &self,
        engine: &EngineState,
        kind: &str,
        surface_id: u32,
        params: &Value,
    ) -> anyhow::Result<Box<dyn crate::model::Surface>> {
        let def = engine
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
