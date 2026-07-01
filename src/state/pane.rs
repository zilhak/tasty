use serde_json::Value;

use crate::core::CoreState;
#[cfg(test)]
use crate::model::SplitDirection;

use super::AppState;

/// Helper: tab 내 surface_id 에 해당하는 TerminalSurface 를 찾는다 (downcast).
fn terminal_surface_in_tab(
    tab: &crate::model::Tab,
    surface_id: u32,
) -> Option<&crate::model::TerminalSurface> {
    tab.layout_opt
        .as_ref()?
        .find_surface(surface_id)?
        .as_any()
        .downcast_ref::<crate::model::TerminalSurface>()
}

impl AppState {
    /// Close the focused pane (unsplit). Returns true if a pane was removed.
    pub fn close_active_pane(&mut self, engine: &mut CoreState) -> bool {
        let target_id = self.active_workspace(engine).focused_pane;

        // Collect all (surface_id, persist_id) in the pane being closed for cleanup.
        let mut targets: Vec<(u32, Option<String>)> = Vec::new();
        {
            let ws = self.active_workspace(engine);
            if let Some(pane) = ws.pane_layout().find_pane(target_id) {
                for tab in &pane.tabs {
                    Self::collect_close_targets(tab, engine, &mut targets);
                }
            }
        }

        let ws = self.active_workspace_mut(engine);
        let removed = ws.pane_layout_mut().close_pane(target_id);
        if removed {
            // Update focus to the first available pane
            if let Some(first) = ws.pane_layout().first_pane() {
                ws.focused_pane = first.id;
            }
            // cleanup_surface 직후엔 layout 에서 surface 가 사라져 kind 조회 불가 →
            // 미리 캡쳐. plugin lifecycle 큐에 cleanup_targets 모두 enqueue (R1 분석).
            for (sid, pid) in targets {
                let kind = self.surface_kind(engine, sid);
                self.cleanup_surface(engine, sid, pid);
                self.enqueue_surface_closed(sid, kind, true);
            }
            engine.mark_layout_dirty();
        }
        removed
    }

    /// Close the focused surface. For split tabs, closes the focused surface
    /// within the tab. For single-surface tabs, delegates to close_surface_by_id
    /// which handles tab/pane/workspace cascading.
    pub fn close_active_surface(&mut self, engine: &mut CoreState) -> bool {
        let surface_id;

        if let Some(pane) = self.focused_pane(engine) {
            let tab = match pane.tabs.get(pane.active_tab) {
                Some(t) => t,
                None => return false,
            };
            surface_id = tab.focused_surface;
        } else {
            return false;
        }
        let persist_id = engine
            .terminals
            .scrollback_persist_id(surface_id)
            .map(str::to_string);
        let kind = self.surface_kind(engine, surface_id);
        let split_handled;
        if let Some(pane) = self.focused_pane_mut(engine) {
            let tab = match pane.active_tab_mut() {
                Some(t) => t,
                None => return false,
            };
            if tab.is_split() {
                if !tab.close_surface(surface_id) {
                    return self.close_surface_by_id(engine, surface_id, true);
                }
                split_handled = true;
            } else {
                return self.close_surface_by_id(engine, surface_id, true);
            }
        } else {
            return false;
        }
        if split_handled {
            self.cleanup_surface(engine, surface_id, persist_id);
            self.enqueue_surface_closed(surface_id, kind, true);
            engine.mark_layout_dirty();
        }
        true
    }

    /// Close a specific surface by ID. Cascades up the hierarchy:
    /// surface -> tab -> pane -> workspace as needed.
    /// When `save_snapshot` is true, the closed item is saved for user restore (Ctrl+Shift+T).
    /// Agent/IPC closures should pass false to avoid polluting the user's undo stack.
    pub fn close_surface_by_id(
        &mut self,
        engine: &mut CoreState,
        surface_id: u32,
        is_user_close: bool,
    ) -> bool {
        self.close_surface_by_id_inner(engine, surface_id, true, is_user_close)
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
        engine: &mut CoreState,
        surface_id: u32,
        is_user_close: bool,
    ) -> bool {
        let closed = self.close_surface_by_id_inner(engine, surface_id, false, is_user_close);
        if closed && engine.workspaces.is_empty() {
            // free fn 으로 직접 호출 — Core 통과 없이 invariant 복구 (apply_create_workspace_inner
            // 은 engine-only 라 self/Core 의존 없음). 본 호출처는 PTY exit cleanup
            // (cascade_terminal_process_exited) 과 egui 의 diff close 두 곳에서 도달 — 둘 다
            // *시스템 invariant restorer* 라 host event 발화 불필요.
            match crate::core::apply_create_workspace_inner(
                engine,
                None,
                "terminal".to_string(),
                serde_json::Value::Null,
                None,
                None,
                None,
                None,
            ) {
                Ok(crate::core::intent::CoreEvent::WorkspaceCreated { index, .. }) => {
                    self.active_workspace = index;
                }
                Ok(_) => unreachable!("apply_create_workspace_inner 는 WorkspaceCreated 만 반환"),
                Err(e) => tracing::warn!(
                    "close_surface_by_id_no_snapshot: auto-recreate workspace failed: {e}"
                ),
            }
        }
        closed
    }

    fn close_surface_by_id_inner(
        &mut self,
        engine: &mut CoreState,
        surface_id: u32,
        save_snapshot: bool,
        is_user_close: bool,
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
                if terminal_surface_in_tab(tab, surface_id).is_some() {
                    let snapshot = crate::model::closed_item::ClosedSurface::from_surface_id(
                        surface_id,
                        engine.terminals.get(surface_id),
                    );
                    let tab_name = tab.display_name().to_string();
                    engine.push_closed_item(crate::model::ClosedItem::Surface {
                        surface: snapshot,
                        tab_name,
                    });
                }
            }
            // close 이전에 leaf surface 의 persist_id 를 추출해 둔다.
            let persist_id = engine
                .terminals
                .scrollback_persist_id(surface_id)
                .map(str::to_string);
            let kind = self.surface_kind(engine, surface_id);
            let ws = &mut engine.workspaces[ws_idx];
            let pane = ws.pane_layout_mut().find_pane_mut(pane_id).unwrap();
            let tab = &mut pane.tabs[tab_idx];
            if tab.close_surface(surface_id) {
                self.cleanup_surface(engine, surface_id, persist_id);
                self.enqueue_surface_closed(surface_id, kind, is_user_close);
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
                        let terminals = &engine.terminals;
                        crate::model::closed_item::ClosedTab::from_tab(
                            &pane.tabs[tab_idx],
                            &mut snap_fn,
                            &|id| terminals.get(id),
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
                    Self::collect_close_targets(&pane.tabs[tab_idx], engine, &mut targets);
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
                    let kind = self.surface_kind(engine, sid);
                    self.cleanup_surface(engine, sid, pid);
                    self.enqueue_surface_closed(sid, kind, is_user_close);
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
                if ws.pane_layout().all_pane_ids().len() > 1
                    && let Some(pane) = ws.pane_layout().find_pane(pane_id)
                {
                    for tab in &pane.tabs {
                        Self::collect_close_targets(tab, engine, &mut targets);
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
                    let kind = self.surface_kind(engine, sid);
                    self.cleanup_surface(engine, sid, pid);
                    self.enqueue_surface_closed(sid, kind, is_user_close);
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
                let terminals = &engine.terminals;
                crate::model::ClosedItem::from_workspace(ws, &mut snap_fn, &|id| terminals.get(id))
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
                        Self::collect_close_targets(tab, engine, &mut targets);
                    }
                }
            }
        }
        // workspaces.remove 이후엔 surface_kind 조회 불가 → 미리 캡쳐.
        let target_kinds: Vec<Option<&'static str>> = targets
            .iter()
            .map(|(sid, _)| self.surface_kind(engine, *sid))
            .collect();
        let workspace_id = engine.workspaces[ws_idx].id;
        engine.workspaces.remove(ws_idx);
        if self.active_workspace >= engine.workspaces.len() && !engine.workspaces.is_empty() {
            self.active_workspace = engine.workspaces.len() - 1;
        }
        // Workspace scope 의 memory entry 정리 (마지막 surface 가 닫혀 workspace 도 사라지는 경로).
        let ws_scope = tasty_memory::Scope::Workspace(workspace_id);
        match self.with_memory(|m| m.purge_scope(&ws_scope)) {
            Ok(stats) if stats.regular + stats.secret > 0 => tracing::debug!(
                workspace_id,
                regular = stats.regular,
                secret = stats.secret,
                "memory: purged closed-workspace scope",
            ),
            Ok(_) => {}
            Err(e) => tracing::warn!(workspace_id, "memory: purge_scope failed: {e}"),
        }
        for ((sid, pid), kind) in targets.into_iter().zip(target_kinds) {
            self.cleanup_surface(engine, sid, pid);
            self.enqueue_surface_closed(sid, kind, is_user_close);
        }
        engine.mark_layout_dirty();
        true
    }
}

#[cfg(test)]
impl AppState {
    /// Test-only helper: split the focused pane along the given direction.
    ///
    /// Production code uses `DomainIntent::SplitPane` dispatched through `Core`.
    /// 테스트 setup 편의 용도로만 직접 호출한다.
    pub(crate) fn test_split_pane(
        &mut self,
        engine: &mut CoreState,
        direction: SplitDirection,
    ) -> anyhow::Result<()> {
        let cwd = self.resolve_inherit_cwd(engine);
        let new_pane_id = engine.next_ids.next_pane();
        let new_tab_id = engine.next_ids.next_tab();
        let new_surface_id = engine.next_ids.next_surface();
        let cols = engine.default_cols;
        let rows = engine.default_rows;

        let sh = crate::core::state::ShellConfig::from_settings(&engine.settings);
        let terminal = crate::model::Pane::spawn_terminal(
            new_surface_id,
            crate::model::ShellSpawnOpts {
                cols,
                rows,
                shell: sh.shell_ref(),
                shell_args: &sh.args_ref(),
                waker: engine.make_waker(new_surface_id),
                working_dir: cwd.as_deref(),
            },
        )?;
        engine.terminals.insert(new_surface_id, terminal);
        let new_pane =
            crate::model::Pane::new_with_terminal_marker(new_pane_id, new_tab_id, new_surface_id);

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
}

/// kind+params로부터 합리적인 탭 표시명을 도출한다.
/// 경로/URL이 있으면 마지막 segment, 없으면 kind에 대응되는 정적 이름을 사용한다.
pub(crate) fn default_tab_name_for_kind(kind: &str, params: &Value) -> String {
    fn basename_or(path: &str, fallback: &str) -> String {
        path.split(['/', '\\'])
            .rfind(|s| !s.is_empty())
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
