//! `Core` — surface/pane/tab/workspace close cascade. `src/core/mod.rs` 의 `impl Core` 분할.

use super::*;

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

/// surface close cascade 의 Step 1 판정 결과 — C2(`apply_close_surface`) /
/// C3(`close_surface_by_id_inner`) 공유.
pub(crate) struct SurfaceCloseLocation {
    pub(crate) ws_idx: usize,
    pub(crate) pane_id: u32,
    pub(crate) tab_idx: usize,
    pub(crate) surface_is_sole_in_tab: bool,
    pub(crate) can_close_surface_in_group: bool,
}

/// surface 를 담은 ws/pane/tab 을 찾고 sole/split 판정. 순수 조회(뮤테이션 없음)라
/// C2/C3 공유. 못 찾으면 None (caller 는 not_found / false 로 귀결).
pub(crate) fn locate_surface_in_pane(
    engine: &crate::core::CoreState,
    surface_id: u32,
) -> Option<SurfaceCloseLocation> {
    let (ws_idx, pane_id) = engine.find_workspace_index_for_surface(surface_id)?;
    let ws = &engine.workspaces[ws_idx];
    let pane = ws.pane_layout().find_pane(pane_id)?;
    let mut found_tab = None;
    for (i, tab) in pane.tabs.iter().enumerate() {
        if tab.contains_surface(surface_id) {
            found_tab = Some(i);
            break;
        }
    }
    let tab_idx = found_tab?;
    let tab = &pane.tabs[tab_idx];
    let surface_is_sole_in_tab;
    let can_close_surface_in_group;
    if tab.is_split() {
        surface_is_sole_in_tab = false;
        can_close_surface_in_group = !matches!(tab.layout(), crate::model::SurfaceLayout::Leaf(_));
    } else if tab.contains_surface(surface_id) {
        surface_is_sole_in_tab = true;
        can_close_surface_in_group = false;
    } else {
        return None;
    }
    Some(SurfaceCloseLocation {
        ws_idx,
        pane_id,
        tab_idx,
        surface_is_sole_in_tab,
        can_close_surface_in_group,
    })
}

/// surface 를 못 찾았을 때의 빈 cascade(`closed=false`). C2 전용.
pub(crate) fn surface_close_not_found(surface_id: u32) -> CoreEvent {
    CoreEvent::SurfaceClosed {
        surface_id,
        closed: false,
        cascade_level: crate::core::intent::CascadeLevel::Surface,
        cleanup_targets: vec![],
        closed_tab_ids: vec![],
        closed_pane_ids: vec![],
        workspace_id_purged: None,
        workspaces_now_empty: false,
    }
}

impl Core {
    /// `DomainIntent::ClosePane` 본문. pane_id 로 모든 workspace 순회.
    /// cleanup_targets 수집 → pane tree close → workspace 안 focused_pane 보정
    /// (닫힌 곳의 자연 이동, 원칙 위반 아님). cleanup_surface 는 cascade.
    pub(super) fn apply_close_pane(engine: &mut crate::core::CoreState, pane_id: u32) -> CoreEvent {
        let ws_idx = match engine.find_workspace_index_for_pane(pane_id) {
            Some(idx) => idx,
            None => {
                return CoreEvent::PaneClosed {
                    pane_id,
                    closed: false,
                    cleanup_targets: vec![],
                };
            }
        };

        let mut targets: Vec<(u32, Option<String>)> = Vec::new();
        if let Some(pane) = engine.workspaces[ws_idx].pane_layout().find_pane(pane_id) {
            for tab in &pane.tabs {
                crate::state::AppState::collect_close_targets(tab, engine, &mut targets);
            }
        }

        let ws = &mut engine.workspaces[ws_idx];
        let was_focused = ws.focused_pane == pane_id;
        let removed = ws.pane_layout_mut().close_pane(pane_id);
        if removed {
            if was_focused && let Some(first) = ws.pane_layout().first_pane() {
                ws.focused_pane = first.id;
            }
            engine.mark_layout_dirty();
        }
        CoreEvent::PaneClosed {
            pane_id,
            closed: removed,
            cleanup_targets: if removed { targets } else { vec![] },
        }
    }

    /// `DomainIntent::CloseSurface` 본문. cascading close — surface→tab→pane→
    /// workspace 단계까지 자동 cascade. 옛 `close_surface_by_id_inner` 의 4-case
    /// 코드 이동. cleanup_surface / memory purge / active_workspace 보정 /
    /// auto-recreate 는 cascade + caller 책임.
    /// surface→tab→pane→workspace cascade close 디스패처. Step1 판정
    /// (`locate_surface_in_pane`)으로 위치를 잡고 case1..4 헬퍼에 순차 위임한다.
    pub(super) fn apply_close_surface(
        engine: &mut crate::core::CoreState,
        surface_id: u32,
        save_snapshot: bool,
    ) -> CoreEvent {
        let loc = match locate_surface_in_pane(engine, surface_id) {
            Some(l) => l,
            None => return surface_close_not_found(surface_id),
        };
        if !loc.surface_is_sole_in_tab && loc.can_close_surface_in_group {
            return Self::close_case_split(engine, &loc, surface_id, save_snapshot)
                .unwrap_or_else(|| surface_close_not_found(surface_id));
        }
        if let Some(ev) = Self::close_case_tab(engine, &loc, surface_id, save_snapshot) {
            return ev;
        }
        if let Some(ev) = Self::close_case_pane(engine, &loc, surface_id, save_snapshot) {
            return ev;
        }
        Self::close_case_workspace(engine, &loc, surface_id, save_snapshot)
    }

    /// Case 1: split tab 안 surface 다중 close. Some=닫힘, None=close 실패(→not_found).
    fn close_case_split(
        engine: &mut crate::core::CoreState,
        loc: &SurfaceCloseLocation,
        surface_id: u32,
        save_snapshot: bool,
    ) -> Option<CoreEvent> {
        use crate::core::intent::CascadeLevel;
        if save_snapshot {
            let tab_name_opt = {
                let ws = &engine.workspaces[loc.ws_idx];
                let pane = ws.pane_layout().find_pane(loc.pane_id).unwrap();
                let tab = &pane.tabs[loc.tab_idx];
                if terminal_surface_in_tab(tab, surface_id).is_some() {
                    Some(tab.display_name().to_string())
                } else {
                    None
                }
            };
            if let Some(tab_name) = tab_name_opt {
                let snapshot = crate::model::closed_item::ClosedSurface::from_surface_id(
                    surface_id,
                    engine.terminals.get(surface_id),
                );
                engine.push_closed_item(crate::model::ClosedItem::Surface {
                    surface: snapshot,
                    tab_name,
                });
            }
        }
        let persist_id = engine
            .terminals
            .scrollback_persist_id(surface_id)
            .map(str::to_string);
        let ws = &mut engine.workspaces[loc.ws_idx];
        let pane = ws.pane_layout_mut().find_pane_mut(loc.pane_id).unwrap();
        let tab = &mut pane.tabs[loc.tab_idx];
        let closed = tab.close_surface(surface_id);
        // 닫힌 surface 가 이 탭의 focused 였다면 close_surface 가 focused_surface 를
        // 재배정한다 (배경 탭도 IPC 포커스 독립으로 여기 도달). 새 focused 의 title
        // 로 탭 제목을 재투영해 죽은 surface 의 title 이 남지 않게 한다.
        let new_focused = tab.focused_surface;
        if closed {
            engine.mark_layout_dirty();
            engine.refresh_tab_osc_title(new_focused);
            return Some(CoreEvent::SurfaceClosed {
                surface_id,
                closed: true,
                cascade_level: CascadeLevel::Surface,
                cleanup_targets: vec![(surface_id, persist_id)],
                closed_tab_ids: vec![],
                closed_pane_ids: vec![],
                workspace_id_purged: None,
                workspaces_now_empty: false,
            });
        }
        None
    }

    /// Case 2: sole surface tab, pane.tabs.len() > 1 — tab close. None=조건 불충족(fallthrough).
    fn close_case_tab(
        engine: &mut crate::core::CoreState,
        loc: &SurfaceCloseLocation,
        surface_id: u32,
        save_snapshot: bool,
    ) -> Option<CoreEvent> {
        use crate::core::intent::CascadeLevel;
        if save_snapshot {
            let ws = &engine.workspaces[loc.ws_idx];
            let pane = ws.pane_layout().find_pane(loc.pane_id).unwrap();
            if pane.tabs.len() > 1 {
                let snapshot_opt = {
                    let mut snap_fn =
                        crate::core::surface_registry::snapshot_fn_for(&engine.surface_registry);
                    let terminals = &engine.terminals;
                    crate::model::closed_item::ClosedTab::from_tab(
                        &pane.tabs[loc.tab_idx],
                        &mut snap_fn,
                        &|id| terminals.get(id),
                    )
                };
                if let Some(snapshot) = snapshot_opt {
                    engine.push_closed_item(crate::model::ClosedItem::Tab(snapshot));
                }
            }
        }
        let mut targets: Vec<(u32, Option<String>)> = Vec::new();
        {
            let ws = &engine.workspaces[loc.ws_idx];
            let pane = ws.pane_layout().find_pane(loc.pane_id).unwrap();
            if pane.tabs.len() > 1 {
                crate::state::AppState::collect_close_targets(
                    &pane.tabs[loc.tab_idx],
                    engine,
                    &mut targets,
                );
            }
        }
        let ws = &mut engine.workspaces[loc.ws_idx];
        let pane = ws.pane_layout_mut().find_pane_mut(loc.pane_id).unwrap();
        if pane.tabs.len() > 1 {
            let closed_tab_id = pane.tabs[loc.tab_idx].id;
            pane.tabs.remove(loc.tab_idx);
            if pane.active_tab >= pane.tabs.len() {
                pane.active_tab = pane.tabs.len() - 1;
            }
            engine.mark_layout_dirty();
            return Some(CoreEvent::SurfaceClosed {
                surface_id,
                closed: true,
                cascade_level: CascadeLevel::Tab,
                cleanup_targets: targets,
                closed_tab_ids: vec![closed_tab_id],
                closed_pane_ids: vec![],
                workspace_id_purged: None,
                workspaces_now_empty: false,
            });
        }
        None
    }

    /// Case 3: last tab in pane, ws 안 pane >1 — pane close. None=fallthrough.
    fn close_case_pane(
        engine: &mut crate::core::CoreState,
        loc: &SurfaceCloseLocation,
        surface_id: u32,
        save_snapshot: bool,
    ) -> Option<CoreEvent> {
        use crate::core::intent::CascadeLevel;
        // Capture pane snapshot before removing (user actions only). Split
        // context(sibling/direction/ratio/side)는 `close_pane`이 트리를
        // 재배치하기 *전*에 캡처해야 한다 — 제거 후엔 부모 Split 노드 자체가
        // 사라져 복구할 수 없다.
        if save_snapshot {
            let ws = &engine.workspaces[loc.ws_idx];
            if ws.pane_layout().all_pane_ids().len() > 1
                && let Some(pane) = ws.pane_layout().find_pane(loc.pane_id)
                && let Some((direction, ratio, was_first, sibling_pane_id)) =
                    ws.pane_layout().locate_split_context(loc.pane_id)
            {
                let snapshot = {
                    let mut snap_fn =
                        crate::core::surface_registry::snapshot_fn_for(&engine.surface_registry);
                    let terminals = &engine.terminals;
                    crate::model::ClosedItem::from_pane(
                        pane,
                        sibling_pane_id,
                        direction,
                        ratio,
                        was_first,
                        &mut snap_fn,
                        &|id| terminals.get(id),
                    )
                };
                engine.push_closed_item(snapshot);
            }
        }
        let mut targets: Vec<(u32, Option<String>)> = Vec::new();
        let mut closed_tab_ids: Vec<u32> = Vec::new();
        {
            let ws = &engine.workspaces[loc.ws_idx];
            if ws.pane_layout().all_pane_ids().len() > 1
                && let Some(pane) = ws.pane_layout().find_pane(loc.pane_id)
            {
                for tab in &pane.tabs {
                    crate::state::AppState::collect_close_targets(tab, engine, &mut targets);
                    closed_tab_ids.push(tab.id);
                }
            }
        }
        let ws = &mut engine.workspaces[loc.ws_idx];
        if ws.pane_layout().all_pane_ids().len() > 1 {
            ws.pane_layout_mut().close_pane(loc.pane_id);
            if let Some(first) = ws.pane_layout().first_pane() {
                ws.focused_pane = first.id;
            }
            engine.mark_layout_dirty();
            return Some(CoreEvent::SurfaceClosed {
                surface_id,
                closed: true,
                cascade_level: CascadeLevel::Pane,
                cleanup_targets: targets,
                closed_tab_ids,
                closed_pane_ids: vec![loc.pane_id],
                workspace_id_purged: None,
                workspaces_now_empty: false,
            });
        }
        None
    }

    /// Case 4: last pane in workspace — workspace close. 항상 SurfaceClosed.
    fn close_case_workspace(
        engine: &mut crate::core::CoreState,
        loc: &SurfaceCloseLocation,
        surface_id: u32,
        save_snapshot: bool,
    ) -> CoreEvent {
        use crate::core::intent::CascadeLevel;
        if save_snapshot {
            let item = {
                let mut snap_fn =
                    crate::core::surface_registry::snapshot_fn_for(&engine.surface_registry);
                let ws = &engine.workspaces[loc.ws_idx];
                let terminals = &engine.terminals;
                crate::model::ClosedItem::from_workspace(ws, &mut snap_fn, &|id| terminals.get(id))
            };
            engine.push_closed_item(item);
        }
        let mut targets: Vec<(u32, Option<String>)> = Vec::new();
        let mut closed_tab_ids: Vec<u32> = Vec::new();
        let mut closed_pane_ids: Vec<u32> = Vec::new();
        {
            let ws = &engine.workspaces[loc.ws_idx];
            for pid in ws.pane_layout().all_pane_ids() {
                closed_pane_ids.push(pid);
                if let Some(pane) = ws.pane_layout().find_pane(pid) {
                    for tab in &pane.tabs {
                        crate::state::AppState::collect_close_targets(tab, engine, &mut targets);
                        closed_tab_ids.push(tab.id);
                    }
                }
            }
        }
        let workspace_id = engine.workspaces[loc.ws_idx].id;
        engine.workspaces.remove(loc.ws_idx);
        let workspaces_now_empty = engine.workspaces.is_empty();
        engine.mark_layout_dirty();

        CoreEvent::SurfaceClosed {
            surface_id,
            closed: true,
            cascade_level: CascadeLevel::Workspace,
            cleanup_targets: targets,
            closed_tab_ids,
            closed_pane_ids,
            workspace_id_purged: Some(workspace_id),
            workspaces_now_empty,
        }
    }

    /// `DomainIntent::CloseTab` 본문. tab 위치 + cleanup_targets 수집 →
    /// pane.close_tab_by_id → mark_layout_dirty. cleanup_surface (AppState
    /// 데이터) 는 cascade 가 처리한다.
    pub(super) fn apply_close_tab(engine: &mut crate::core::CoreState, tab_id: u32) -> CoreEvent {
        let mut targets: Vec<(u32, Option<String>)> = Vec::new();
        let mut found_pane_id = None;
        for workspace in &engine.workspaces {
            for &pid in &workspace.pane_layout().all_pane_ids() {
                if let Some(pane) = workspace.pane_layout().find_pane(pid)
                    && let Some(tab) = pane.tabs.iter().find(|t| t.id == tab_id)
                {
                    crate::state::AppState::collect_close_targets(tab, engine, &mut targets);
                    found_pane_id = Some(pid);
                    break;
                }
            }
            if found_pane_id.is_some() {
                break;
            }
        }

        let pane_id = match found_pane_id {
            Some(pid) => pid,
            None => {
                return CoreEvent::TabClosed {
                    tab_id,
                    pane_id: None,
                    closed: false,
                    cleanup_targets: vec![],
                };
            }
        };

        let closed = engine
            .find_pane_by_id_mut(pane_id)
            .map(|p| p.close_tab_by_id(tab_id))
            .unwrap_or(false);
        if closed {
            engine.mark_layout_dirty();
        }
        CoreEvent::TabClosed {
            tab_id,
            pane_id: Some(pane_id),
            closed,
            cleanup_targets: if closed { targets } else { vec![] },
        }
    }
}

#[cfg(test)]
mod close_surface_cascade_tests {
    //! `apply_close_surface` (C2) 의 반환 `CoreEvent::SurfaceClosed` 필드
    //! characterization. Case2(tab)/Case3(pane)/Case4(workspace) cascade 의
    //! `cleanup_targets`·`closed_tab_ids`·`closed_pane_ids`·`workspace_id_purged`·
    //! `workspaces_now_empty`·`cascade_level` 을 고정한다. 필드 하나라도 누락되면
    //! caller `cascade_surface_closed` 가 plugin lifecycle 큐·host TabClosed·
    //! memory purge 를 건너뛰어 런타임에서만 드러나는 leak 이 되므로, case별 헬퍼
    //! 추출 리팩터의 안전망이다. save_snapshot=false 로 호출해 undo 스택/스냅샷
    //! 경로는 배제하고 순수 cascade 반환값만 고정한다.
    use super::*;
    use crate::core::intent::CascadeLevel;
    use tasty_terminal::Terminal;

    fn test_engine() -> CoreState {
        let waker: tasty_terminal::Waker = std::sync::Arc::new(|| {});
        CoreState::new(80, 24, waker).expect("engine")
    }

    fn insert_detached(engine: &mut CoreState, sid: u32) {
        engine.terminals.insert(sid, Terminal::new_detached(80, 24));
    }

    /// Case 2: sole-surface tab & pane 에 tab >1 → tab close.
    #[test]
    fn case2_tab_close_returns_tab_level_fields() {
        let mut engine = test_engine();
        let sid0 = engine.workspaces[0].all_surface_ids()[0];
        let (ws_idx, pane_id) = engine.find_workspace_index_for_surface(sid0).unwrap();
        // 두 번째 탭(sole surface) 추가.
        let tab1_id = engine.next_ids.next_tab();
        let sid1 = engine.next_ids.next_surface();
        insert_detached(&mut engine, sid1);
        engine.workspaces[ws_idx]
            .pane_layout_mut()
            .find_pane_mut(pane_id)
            .unwrap()
            .add_terminal_marker_tab(tab1_id, sid1);

        let ev = Core::apply_close_surface(&mut engine, sid1, false);
        match ev {
            CoreEvent::SurfaceClosed {
                closed,
                cascade_level,
                cleanup_targets,
                closed_tab_ids,
                closed_pane_ids,
                workspace_id_purged,
                workspaces_now_empty,
                ..
            } => {
                assert!(closed);
                assert_eq!(cascade_level, CascadeLevel::Tab);
                assert_eq!(closed_tab_ids, vec![tab1_id]);
                assert_eq!(cleanup_targets, vec![(sid1, None)]);
                assert!(closed_pane_ids.is_empty());
                assert_eq!(workspace_id_purged, None);
                assert!(!workspaces_now_empty);
            }
            other => panic!("expected SurfaceClosed, got {other:?}"),
        }
        assert_eq!(
            engine.workspaces[ws_idx]
                .pane_layout()
                .find_pane(pane_id)
                .unwrap()
                .tabs
                .len(),
            1
        );
    }

    /// Case 3: last tab in pane & ws 에 pane >1 → pane close.
    #[test]
    fn case3_pane_close_returns_pane_level_fields() {
        let mut engine = test_engine();
        let sid0 = engine.workspaces[0].all_surface_ids()[0];
        let (ws_idx, pane0) = engine.find_workspace_index_for_surface(sid0).unwrap();
        let pane1_id = engine.next_ids.next_pane();
        let tab1_id = engine.next_ids.next_tab();
        let sid1 = engine.next_ids.next_surface();
        insert_detached(&mut engine, sid1);
        let new_pane = crate::model::Pane::new_with_terminal_marker(pane1_id, tab1_id, sid1);
        let leftover = engine.workspaces[ws_idx]
            .pane_layout_mut()
            .split_pane_in_place(pane0, crate::model::SplitDirection::Horizontal, new_pane);
        assert!(leftover.is_none(), "split 성공해야 함");

        let ev = Core::apply_close_surface(&mut engine, sid1, false);
        match ev {
            CoreEvent::SurfaceClosed {
                closed,
                cascade_level,
                cleanup_targets,
                closed_tab_ids,
                closed_pane_ids,
                workspace_id_purged,
                workspaces_now_empty,
                ..
            } => {
                assert!(closed);
                assert_eq!(cascade_level, CascadeLevel::Pane);
                assert_eq!(closed_pane_ids, vec![pane1_id]);
                assert_eq!(closed_tab_ids, vec![tab1_id]);
                assert_eq!(cleanup_targets, vec![(sid1, None)]);
                assert_eq!(workspace_id_purged, None);
                assert!(!workspaces_now_empty);
            }
            other => panic!("expected SurfaceClosed, got {other:?}"),
        }
        assert_eq!(
            engine.workspaces[ws_idx].pane_layout().all_pane_ids().len(),
            1
        );
    }

    /// Case 4: last pane in workspace, 다른 workspace 생존 → workspace close.
    #[test]
    fn case4_workspace_close_returns_workspace_level_fields() {
        let mut engine = test_engine();
        let ws1_id = engine.next_ids.next_workspace();
        let pane1_id = engine.next_ids.next_pane();
        let tab1_id = engine.next_ids.next_tab();
        let sid1 = engine.next_ids.next_surface();
        insert_detached(&mut engine, sid1);
        let ws1 = crate::model::Workspace::new_with_terminal_marker(
            ws1_id,
            "ws1".to_string(),
            pane1_id,
            tab1_id,
            sid1,
        );
        engine.workspaces.push(ws1);
        assert_eq!(engine.workspaces.len(), 2);

        let ev = Core::apply_close_surface(&mut engine, sid1, false);
        match ev {
            CoreEvent::SurfaceClosed {
                closed,
                cascade_level,
                cleanup_targets,
                closed_tab_ids,
                closed_pane_ids,
                workspace_id_purged,
                workspaces_now_empty,
                ..
            } => {
                assert!(closed);
                assert_eq!(cascade_level, CascadeLevel::Workspace);
                assert_eq!(workspace_id_purged, Some(ws1_id));
                assert_eq!(closed_pane_ids, vec![pane1_id]);
                assert_eq!(closed_tab_ids, vec![tab1_id]);
                assert_eq!(cleanup_targets, vec![(sid1, None)]);
                assert!(!workspaces_now_empty);
            }
            other => panic!("expected SurfaceClosed, got {other:?}"),
        }
        assert_eq!(engine.workspaces.len(), 1);
    }

    /// Case 4 변형: 마지막 workspace 를 닫으면 `workspaces_now_empty==true`.
    #[test]
    fn case4_last_workspace_reports_now_empty() {
        let mut engine = test_engine();
        let sid0 = engine.workspaces[0].all_surface_ids()[0];
        insert_detached(&mut engine, sid0);
        let ws0_id = engine.workspaces[0].id;

        let ev = Core::apply_close_surface(&mut engine, sid0, false);
        match ev {
            CoreEvent::SurfaceClosed {
                closed,
                cascade_level,
                workspace_id_purged,
                workspaces_now_empty,
                ..
            } => {
                assert!(closed);
                assert_eq!(cascade_level, CascadeLevel::Workspace);
                assert_eq!(workspace_id_purged, Some(ws0_id));
                assert!(workspaces_now_empty);
            }
            other => panic!("expected SurfaceClosed, got {other:?}"),
        }
        assert!(engine.workspaces.is_empty());
    }

    /// Case 1 보강: split tab 다중 close 의 반환 필드(기존 title-재투영 테스트는 미검증).
    #[test]
    fn case1_split_close_returns_single_cleanup_target() {
        let mut engine = test_engine();
        let sid_a = engine.workspaces[0].all_surface_ids()[0];
        insert_detached(&mut engine, sid_a);
        let (ws_idx, pane_id) = engine.find_workspace_index_for_surface(sid_a).unwrap();
        let sid_b = engine.next_ids.next_surface();
        engine.workspaces[ws_idx]
            .pane_layout_mut()
            .find_pane_mut(pane_id)
            .unwrap()
            .split_surface_by_id_marker(sid_a, crate::model::SplitDirection::Horizontal, sid_b)
            .unwrap();
        insert_detached(&mut engine, sid_b);

        let ev = Core::apply_close_surface(&mut engine, sid_a, false);
        match ev {
            CoreEvent::SurfaceClosed {
                closed,
                cascade_level,
                cleanup_targets,
                closed_tab_ids,
                closed_pane_ids,
                workspace_id_purged,
                ..
            } => {
                assert!(closed);
                assert_eq!(cascade_level, CascadeLevel::Surface);
                assert_eq!(cleanup_targets, vec![(sid_a, None)]);
                assert!(closed_tab_ids.is_empty());
                assert!(closed_pane_ids.is_empty());
                assert_eq!(workspace_id_purged, None);
            }
            other => panic!("expected SurfaceClosed, got {other:?}"),
        }
    }
}
