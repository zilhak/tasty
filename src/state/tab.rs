use serde_json::Value;
#[cfg(test)]
use serde_json::json;

use super::AppState;
use crate::core::CoreState;

impl AppState {
    /// Add a new tab in the focused pane.
    pub fn add_tab(&mut self, engine: &mut CoreState) -> anyhow::Result<()> {
        let cwd = self.resolve_inherit_cwd(engine);
        let tab_id = engine.next_ids.next_tab();
        let surface_id = engine.next_ids.next_surface();
        let cols = engine.default_cols;
        let rows = engine.default_rows;
        let sh = crate::core::state::ShellConfig::from_settings(&engine.settings);
        let waker = engine.make_waker(surface_id);
        let terminal = crate::model::Pane::spawn_terminal(
            surface_id,
            crate::model::ShellSpawnOpts {
                cols,
                rows,
                shell: sh.shell_ref(),
                shell_args: &sh.args_ref(),
                waker,
                working_dir: cwd.as_deref(),
            },
        )?;
        engine.terminals.insert(surface_id, terminal);
        if let Some(pane) = self.focused_pane_mut(engine) {
            pane.add_terminal_marker_tab(tab_id, surface_id);
        }
        engine.send_fast_init(surface_id);
        engine.mark_layout_dirty();
        Ok(())
    }

    /// Generic kind+params 기반 탭 추가. SurfaceKindRegistry를 통해 surface를 만들고
    /// 포커스된 pane에 부착한다. Returns (tab_id, surface_id) on success.
    pub fn add_kind_tab(
        &mut self,
        engine: &mut CoreState,
        kind: &str,
        params: &Value,
    ) -> anyhow::Result<(u32, u32)> {
        let tab_id = engine.next_ids.next_tab();
        let surface_id = engine.next_ids.next_surface();
        let cwd = self.resolve_inherit_cwd(engine);
        let surface =
            engine.create_surface_via_registry(kind, surface_id, cwd.as_deref(), params)?;
        let name = super::pane::default_tab_name_for_kind(kind, params);
        if let Some(pane) = self.focused_pane_mut(engine) {
            pane.add_surface_tab(tab_id, name, None, surface);
            engine.mark_layout_dirty();
            Ok((tab_id, surface_id))
        } else {
            anyhow::bail!("no focused pane to add tab to")
        }
    }

    /// Add an empty placeholder tab in the focused pane. Returns (tab_id, surface_id).
    pub fn add_empty_tab(&mut self, engine: &mut CoreState) -> Option<(u32, u32)> {
        self.add_kind_tab(engine, "empty", &Value::Null).ok()
    }

    /// Next tab in the focused pane.
    pub fn next_tab_in_pane(&mut self, engine: &mut CoreState) {
        if let Some(pane) = self.focused_pane_mut(engine) {
            pane.next_tab();
        }
    }

    /// Previous tab in the focused pane.
    pub fn prev_tab_in_pane(&mut self, engine: &mut CoreState) {
        if let Some(pane) = self.focused_pane_mut(engine) {
            pane.prev_tab();
        }
    }

    /// Go to tab by index (0-based) in the focused pane.
    pub fn goto_tab_in_pane(&mut self, engine: &mut CoreState, index: usize) -> bool {
        if let Some(pane) = self.focused_pane_mut(engine) {
            pane.goto_tab(index)
        } else {
            false
        }
    }

    /// Close the active tab in the focused pane. Returns true if a tab was closed.
    pub fn close_active_tab(&mut self, engine: &mut CoreState) -> bool {
        // Capture tab snapshot + collect persist_ids (immutable borrow).
        let mut targets: Vec<(u32, Option<String>)> = Vec::new();
        let snapshot_opt = if let Some(pane) = self.focused_pane(engine) {
            let active = pane.active_tab;
            if let Some(tab) = pane.tabs.get(active) {
                super::AppState::collect_close_targets(tab, engine, &mut targets);
                let mut snap_fn =
                    crate::engine::surface_registry::snapshot_fn_for(&engine.surface_registry);
                let terminals = &engine.terminals;
                crate::model::closed_item::ClosedTab::from_tab(tab, &mut snap_fn, &|id| {
                    terminals.get(id)
                })
            } else {
                None
            }
        } else {
            None
        };
        if let Some(snapshot) = snapshot_opt {
            engine.push_closed_item(crate::model::ClosedItem::Tab(snapshot));
        }
        let closed = if let Some(pane) = self.focused_pane_mut(engine) {
            pane.close_active_tab()
        } else {
            false
        };
        if closed {
            // cleanup_surface 직후엔 layout 에서 surface 가 사라져 kind 조회 불가 →
            // 미리 캡쳐. plugin lifecycle 큐에 cleanup_targets 모두 enqueue (R1 분석).
            for (sid, pid) in targets {
                let kind = self.surface_kind(engine, sid);
                self.cleanup_surface(engine, sid, pid);
                self.enqueue_surface_closed(sid, kind, true);
            }
            engine.mark_layout_dirty();
        }
        closed
    }
}

#[cfg(test)]
impl AppState {
    /// Test-only helper: add a Markdown viewer tab in the focused pane.
    ///
    /// Production code dispatches `DomainIntent::NewTab { kind: "markdown" }`
    /// through `Core`. 본 헬퍼는 테스트 setup 편의 용도.
    pub(crate) fn test_add_markdown_tab(
        &mut self,
        engine: &mut CoreState,
        file_path: String,
    ) -> anyhow::Result<()> {
        self.add_kind_tab(engine, "markdown", &json!({"file": file_path}))
            .map(|_| ())
    }

    /// Test-only helper: replace the surface for `surface_id` with a freshly
    /// created surface of `kind` (cleared explicit_name).
    ///
    /// Production path is `DomainIntent::ConvertSurface` via `Core::apply_convert_surface`.
    pub(crate) fn test_convert_surface_to_kind(
        &mut self,
        engine: &mut CoreState,
        surface_id: u32,
        kind: &str,
        params: &Value,
    ) -> bool {
        let new_surface = match engine.create_surface_via_registry(kind, surface_id, None, params) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("test_convert_surface_to_kind('{}') failed: {}", kind, e);
                return false;
            }
        };

        // Locate the tab containing this surface.
        let mut location: Option<(usize, u32, usize)> = None;
        'outer: for (ws_idx, workspace) in engine.workspaces.iter().enumerate() {
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

        let ws = &mut engine.workspaces[ws_idx];
        let pane = match ws.pane_layout_mut().find_pane_mut(pane_id) {
            Some(p) => p,
            None => return false,
        };
        let tab = &mut pane.tabs[tab_idx];

        if tab.is_split() {
            let replaced = tab.layout_mut().replace_surface(surface_id, new_surface);
            if replaced {
                engine.mark_layout_dirty();
            }
            return replaced;
        }
        tab.put_surface(new_surface);
        tab.explicit_name = None;
        engine.mark_layout_dirty();
        true
    }
}
