use serde_json::Value;
#[cfg(test)]
use serde_json::json;

use super::AppState;
use crate::core::CoreState;

impl AppState {
    /// Add a new tab in the focused pane.
    pub fn add_tab(&mut self, engine: &mut CoreState) -> anyhow::Result<()> {
        // mirror 누출 차단 — 로컬 PTY spawn 은 "workspace 전체가 remote" 불변식을
        // 깬다. 가드가 toast 를 띄우고 no-op(Ok) 로 반환한다(2단계에서 원격 forward).
        if self.block_mirror_structural(engine) {
            return Ok(());
        }
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
        // mirror 누출 차단 — 로컬 surface 생성은 "workspace 전체가 remote" 불변식을
        // 깬다. 가드가 toast 를 띄우고 Err 로 반환한다(2단계에서 원격 forward).
        if self.block_mirror_structural(engine) {
            anyhow::bail!("mirror workspace: structural change blocked");
        }
        let tab_id = engine.next_ids.next_tab();
        let surface_id = engine.next_ids.next_surface();
        let cwd = self.resolve_inherit_cwd(engine);
        let surface =
            engine.create_surface_via_registry(kind, surface_id, cwd.as_deref(), params)?;
        let name = super::pane::default_tab_name_for_kind(
            kind,
            params,
            engine.surface_registry.get(kind).as_deref(),
        );
        if let Some(pane) = self.focused_pane_mut(engine) {
            pane.add_surface_tab(tab_id, name, None, surface);
            engine.mark_layout_dirty();
            Ok((tab_id, surface_id))
        } else {
            anyhow::bail!("no focused pane to add tab to")
        }
    }

    /// `add_kind_tab` 의 surface-owner 타겟 변형: focused pane 대신 `owner_surface_id`
    /// 가 속한 pane 에 탭을 추가한다. 우클릭한 explorer 가 focused pane 이 아니어도
    /// (background pane) 그 explorer 가 있는 pane 에 새 탭이 열리도록 해 focused-pane
    /// 의존을 제거한다. Returns (tab_id, surface_id) on success.
    pub fn add_kind_tab_by_owner(
        &mut self,
        engine: &mut CoreState,
        owner_surface_id: u32,
        kind: &str,
        params: &Value,
    ) -> anyhow::Result<(u32, u32)> {
        // owner 가 속한 pane_id 를 활성 워크스페이스에서 찾는다.
        let ws = self.active_workspace(engine);
        let mut target_pane = None;
        for pid in ws.pane_layout().all_pane_ids() {
            if let Some(pane) = ws.pane_layout().find_pane(pid)
                && pane
                    .tabs
                    .iter()
                    .any(|t| t.contains_surface(owner_surface_id))
            {
                target_pane = Some(pid);
                break;
            }
        }
        let Some(pane_id) = target_pane else {
            anyhow::bail!("owner surface {owner_surface_id} not found in active workspace");
        };
        let tab_id = engine.next_ids.next_tab();
        let surface_id = engine.next_ids.next_surface();
        let cwd = self.resolve_inherit_cwd(engine);
        let surface =
            engine.create_surface_via_registry(kind, surface_id, cwd.as_deref(), params)?;
        let name = super::pane::default_tab_name_for_kind(
            kind,
            params,
            engine.surface_registry.get(kind).as_deref(),
        );
        let ws = self.active_workspace_mut(engine);
        if let Some(pane) = ws.pane_layout_mut().find_pane_mut(pane_id) {
            pane.add_surface_tab(tab_id, name, None, surface);
            engine.mark_layout_dirty();
            Ok((tab_id, surface_id))
        } else {
            anyhow::bail!("pane {pane_id} vanished before add_surface_tab");
        }
    }

    /// 대상 explorer surface(`sid`)의 활성 탭 cwd 를 `folder` 로 설정하고(좌측 트리
    /// 루트 이동 + current=folder + 히스토리 초기화) 뷰를 리로드한다. 컨텍스트 메뉴
    /// "이 폴더로 루트 설정" 이 사용. surface_id→패널 탐색은 focus 독립(전 pane 순회).
    pub fn set_explorer_cwd(
        &mut self,
        engine: &mut CoreState,
        sid: u32,
        folder: std::path::PathBuf,
    ) {
        let ws = self.active_workspace_mut(engine);
        let pane_ids = ws.pane_layout().all_pane_ids();
        let mut done = false;
        for pid in pane_ids {
            let Some(pane) = ws.pane_layout_mut().find_pane_mut(pid) else {
                continue;
            };
            for tab in pane.tabs.iter_mut() {
                if !tab.contains_surface(sid) {
                    continue;
                }
                if let Some(leaf) = tab.layout_mut().find_leaf_mut(sid)
                    && let Some(ex) = leaf
                        .as_any_mut()
                        .downcast_mut::<crate::model::ExplorerPanel>()
                {
                    ex.active_tab_mut().set_cwd(folder.clone());
                    done = true;
                }
            }
            if done {
                break;
            }
        }
        // 뷰 리로드 (엔트리 캐시는 explorer_views 에 있어 ws 借用 종료 후 접근).
        // explorer_views 는 gui 전용 뷰 스토어 — headless 엔 뷰가 없어 리로드 불필요.
        #[cfg(feature = "gui")]
        if done && let Some(v) = self.explorer_views.get_mut(sid) {
            v.request_reload();
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

    /// Close a specific tab in a specific pane (context menu 등 임의 (pane_id, tab_index)
    /// 지정 close). focused pane / active tab 와 무관하게 동작한다.
    /// 내부 모든 surface cleanup + closed_item snapshot + layout dirty 마킹을 수행.
    pub fn close_tab(&mut self, engine: &mut CoreState, pane_id: u32, tab_index: usize) -> bool {
        let mut targets: Vec<(u32, Option<String>)> = Vec::new();
        let snapshot_opt = if let Some(pane) = self
            .active_workspace(engine)
            .pane_layout()
            .find_pane(pane_id)
        {
            if let Some(tab) = pane.tabs.get(tab_index) {
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
        let closed = if let Some(pane) = self
            .active_workspace_mut(engine)
            .pane_layout_mut()
            .find_pane_mut(pane_id)
        {
            pane.close_tab(tab_index)
        } else {
            false
        };
        if closed {
            for (sid, pid) in targets {
                let kind = self.surface_kind(engine, sid);
                self.cleanup_surface(engine, sid, pid);
                self.enqueue_surface_closed(sid, kind, true);
            }
            engine.mark_layout_dirty();
        }
        closed
    }

    /// Close the active tab in the focused pane. Returns true if a tab was closed.
    pub fn close_active_tab(&mut self, engine: &mut CoreState) -> bool {
        // mirror 누출 차단 — tab close 는 mirror 트리를 원격과 어긋나게 한다.
        // true 를 돌려 호출부의 close fallback 체인을 멈춘다(전체 mirror 는 유지).
        if self.block_mirror_structural(engine) {
            return true;
        }
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
