//! surface·tab·pane·workspace를 닫고 호출자에게 후속 정리 대상을 반환한다.

use super::*;

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

/// 레이아웃을 지우기 전에 surface ID와 scrollback 저장 ID를 모아 후속 정리에 넘긴다.
pub(crate) fn collect_close_targets(
    tab: &crate::model::Tab,
    engine: &crate::core::CoreState,
    out: &mut Vec<(u32, Option<String>)>,
) {
    tab.for_each_surface(&mut |s| {
        if let Some(ts) = s.as_any().downcast_ref::<crate::model::TerminalSurface>() {
            out.push((
                ts.id,
                engine
                    .terminals
                    .scrollback_persist_id(ts.id)
                    .map(str::to_string),
            ));
        } else if let Some(es) = s.as_any().downcast_ref::<crate::model::EmptySurface>() {
            let pid = es
                .deferred_spawn()
                .and_then(|sp| sp.scrollback_persist_id.clone());
            out.push((es.id, pid));
        } else if let Some(sid) = s.surface_id() {
            // 비터미널도 plugin 자원 정리와 종료 통지가 필요하다.
            out.push((sid, None));
        }
    });
}

pub(crate) struct SurfaceCloseLocation {
    pub(crate) ws_idx: usize,
    pub(crate) pane_id: u32,
    pub(crate) tab_idx: usize,
    pub(crate) surface_is_sole_in_tab: bool,
    pub(crate) can_close_surface_in_group: bool,
}

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

pub(crate) fn surface_close_not_found(surface_id: u32) -> CoreEvent {
    CoreEvent::SurfaceClosed {
        surface_id,
        closed: false,
        cascade_level: crate::core::intent::CascadeLevel::Surface,
        cleanup_targets: vec![],
        closed_tab_ids: vec![],
        closed_pane_ids: vec![],
        workspace_purged: None,
        workspaces_now_empty: false,
    }
}

impl Core {
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
                collect_close_targets(tab, engine, &mut targets);
            }
        }

        let ws = &mut engine.workspaces[ws_idx];
        let removed = ws.close_pane_preserving_focus(pane_id);
        if removed {
            engine.mark_layout_dirty();
        }
        CoreEvent::PaneClosed {
            pane_id,
            closed: removed,
            cleanup_targets: if removed { targets } else { vec![] },
        }
    }

    /// 빈 상위 tab·pane·workspace까지 닫을 수 있다.
    /// 창 자원·메모리 정리와 활성 workspace 보정·대체 workspace 생성은 호출자의 후속 처리다.
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
        // 닫힌 surface의 제목이 남지 않도록 새로 선택된 surface의 제목을 반영한다.
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
                workspace_purged: None,
                workspaces_now_empty: false,
            });
        }
        None
    }

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
                collect_close_targets(&pane.tabs[loc.tab_idx], engine, &mut targets);
            }
        }
        let ws = &mut engine.workspaces[loc.ws_idx];
        let pane = ws.pane_layout_mut().find_pane_mut(loc.pane_id).unwrap();
        if pane.tabs.len() > 1 {
            let closed_tab_id = pane.tabs[loc.tab_idx].id;
            pane.remove_tab_preserving_active(loc.tab_idx);
            engine.mark_layout_dirty();
            return Some(CoreEvent::SurfaceClosed {
                surface_id,
                closed: true,
                cascade_level: CascadeLevel::Tab,
                cleanup_targets: targets,
                closed_tab_ids: vec![closed_tab_id],
                closed_pane_ids: vec![],
                workspace_purged: None,
                workspaces_now_empty: false,
            });
        }
        None
    }

    fn close_case_pane(
        engine: &mut crate::core::CoreState,
        loc: &SurfaceCloseLocation,
        surface_id: u32,
        save_snapshot: bool,
    ) -> Option<CoreEvent> {
        use crate::core::intent::CascadeLevel;
        // pane 제거가 부모 split을 없애므로 복원할 분할 정보는 제거 전에 캡처한다.
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
                    collect_close_targets(tab, engine, &mut targets);
                    closed_tab_ids.push(tab.id);
                }
            }
        }
        let ws = &mut engine.workspaces[loc.ws_idx];
        if ws.pane_layout().all_pane_ids().len() > 1 {
            ws.close_pane_preserving_focus(loc.pane_id);
            engine.mark_layout_dirty();
            return Some(CoreEvent::SurfaceClosed {
                surface_id,
                closed: true,
                cascade_level: CascadeLevel::Pane,
                cleanup_targets: targets,
                closed_tab_ids,
                closed_pane_ids: vec![loc.pane_id],
                workspace_purged: None,
                workspaces_now_empty: false,
            });
        }
        None
    }

    fn close_case_workspace(
        engine: &mut crate::core::CoreState,
        loc: &SurfaceCloseLocation,
        surface_id: u32,
        save_snapshot: bool,
    ) -> CoreEvent {
        use crate::close_trace;
        use crate::core::intent::CascadeLevel;
        use std::time::Instant;

        // 실제 자원 정리가 이 함수 뒤에 이어지므로 전체 측정 시작 시각을 넘긴다.
        let t_close = Instant::now();
        crate::close_trace::arm_cascade(t_close, save_snapshot);
        if save_snapshot {
            let t = Instant::now();
            let item = {
                let mut snap_fn =
                    crate::core::surface_registry::snapshot_fn_for(&engine.surface_registry);
                let ws = &engine.workspaces[loc.ws_idx];
                let terminals = &engine.terminals;
                crate::model::ClosedItem::from_workspace(ws, &mut snap_fn, &|id| terminals.get(id))
            };
            close_trace::log_snapshot(t, &item, "cascade");
            let t = Instant::now();
            engine.push_closed_item(item).log(t.elapsed(), "cascade");
        }
        let t_collect = Instant::now();
        let mut targets: Vec<(u32, Option<String>)> = Vec::new();
        let mut closed_tab_ids: Vec<u32> = Vec::new();
        let mut closed_pane_ids: Vec<u32> = Vec::new();
        {
            let ws = &engine.workspaces[loc.ws_idx];
            for pid in ws.pane_layout().all_pane_ids() {
                closed_pane_ids.push(pid);
                if let Some(pane) = ws.pane_layout().find_pane(pid) {
                    for tab in &pane.tabs {
                        collect_close_targets(tab, engine, &mut targets);
                        closed_tab_ids.push(tab.id);
                    }
                }
            }
        }
        close_trace::log_collect(t_collect, targets.len(), "cascade");
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
            workspace_purged: Some((loc.ws_idx, workspace_id)),
            workspaces_now_empty,
        }
    }

    pub(super) fn apply_close_tab(engine: &mut crate::core::CoreState, tab_id: u32) -> CoreEvent {
        let mut targets: Vec<(u32, Option<String>)> = Vec::new();
        let mut found_pane_id = None;
        for workspace in &engine.workspaces {
            for &pid in &workspace.pane_layout().all_pane_ids() {
                if let Some(pane) = workspace.pane_layout().find_pane(pid)
                    && let Some(tab) = pane.tabs.iter().find(|t| t.id == tab_id)
                {
                    collect_close_targets(tab, engine, &mut targets);
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
    //! snapshot 저장을 끄고 닫기 결과의 후속 처리 대상·ID·계층을 검사한다.
    //! 실제 plugin 자원 회수나 이벤트 전달을 실행하는 검사는 아니다.
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

    #[test]
    fn case2_tab_close_returns_tab_level_fields() {
        let mut engine = test_engine();
        let sid0 = engine.workspaces[0].all_surface_ids()[0];
        let (ws_idx, pane_id) = engine.find_workspace_index_for_surface(sid0).unwrap();
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
                workspace_purged,
                workspaces_now_empty,
                ..
            } => {
                assert!(closed);
                assert_eq!(cascade_level, CascadeLevel::Tab);
                assert_eq!(closed_tab_ids, vec![tab1_id]);
                assert_eq!(cleanup_targets, vec![(sid1, None)]);
                assert!(closed_pane_ids.is_empty());
                assert_eq!(workspace_purged, None);
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
                workspace_purged,
                workspaces_now_empty,
                ..
            } => {
                assert!(closed);
                assert_eq!(cascade_level, CascadeLevel::Pane);
                assert_eq!(closed_pane_ids, vec![pane1_id]);
                assert_eq!(closed_tab_ids, vec![tab1_id]);
                assert_eq!(cleanup_targets, vec![(sid1, None)]);
                assert_eq!(workspace_purged, None);
                assert!(!workspaces_now_empty);
            }
            other => panic!("expected SurfaceClosed, got {other:?}"),
        }
        assert_eq!(
            engine.workspaces[ws_idx].pane_layout().all_pane_ids().len(),
            1
        );
    }

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
                workspace_purged,
                workspaces_now_empty,
                ..
            } => {
                assert!(closed);
                assert_eq!(cascade_level, CascadeLevel::Workspace);
                assert_eq!(workspace_purged.map(|(_, id)| id), Some(ws1_id));
                assert_eq!(closed_pane_ids, vec![pane1_id]);
                assert_eq!(closed_tab_ids, vec![tab1_id]);
                assert_eq!(cleanup_targets, vec![(sid1, None)]);
                assert!(!workspaces_now_empty);
            }
            other => panic!("expected SurfaceClosed, got {other:?}"),
        }
        assert_eq!(engine.workspaces.len(), 1);
    }

    /// 앞쪽 탭 삭제 후에도 active_tab 인덱스가 원래 보던 탭을 가리켜야 한다.
    #[test]
    fn case2_tab_close_preserves_the_viewed_tab() {
        let mut engine = test_engine();
        let sid0 = engine.workspaces[0].all_surface_ids()[0];
        let (ws_idx, pane_id) = engine.find_workspace_index_for_surface(sid0).unwrap();
        let mut tab_ids = vec![];
        for _ in 0..2 {
            let tab_id = engine.next_ids.next_tab();
            let sid = engine.next_ids.next_surface();
            insert_detached(&mut engine, sid);
            engine.workspaces[ws_idx]
                .pane_layout_mut()
                .find_pane_mut(pane_id)
                .unwrap()
                .add_terminal_marker_tab(tab_id, sid);
            tab_ids.push(tab_id);
        }
        let pane = engine.workspaces[ws_idx]
            .pane_layout_mut()
            .find_pane_mut(pane_id)
            .unwrap();
        pane.active_tab = 1;
        let viewed_tab_id = pane.tabs[1].id;

        let ev = Core::apply_close_surface(&mut engine, sid0, false);
        assert!(matches!(ev, CoreEvent::SurfaceClosed { closed: true, .. }));

        let pane = engine.workspaces[ws_idx]
            .pane_layout()
            .find_pane(pane_id)
            .unwrap();
        assert_eq!(pane.tabs.len(), 2);
        assert_eq!(
            pane.tabs[pane.active_tab].id, viewed_tab_id,
            "앞쪽 탭이 닫혀도 보던 탭이 유지돼야 한다"
        );
    }

    #[test]
    fn case3_pane_close_keeps_focus_on_an_untouched_pane() {
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
        assert!(leftover.is_none());
        // 첫 pane으로 무조건 옮기는 오류를 잡으려면 포커스를 다른 pane에 두어야 한다.
        let pane2_id = engine.next_ids.next_pane();
        let tab2_id = engine.next_ids.next_tab();
        let sid2 = engine.next_ids.next_surface();
        insert_detached(&mut engine, sid2);
        let third = crate::model::Pane::new_with_terminal_marker(pane2_id, tab2_id, sid2);
        let leftover = engine.workspaces[ws_idx]
            .pane_layout_mut()
            .split_pane_in_place(pane1_id, crate::model::SplitDirection::Horizontal, third);
        assert!(leftover.is_none());
        assert_eq!(
            engine.workspaces[ws_idx].pane_layout().all_pane_ids().len(),
            3
        );
        engine.workspaces[ws_idx].focused_pane = pane2_id;

        let ev = Core::apply_close_surface(&mut engine, sid0, false);
        assert!(matches!(ev, CoreEvent::SurfaceClosed { closed: true, .. }));

        assert_eq!(
            engine.workspaces[ws_idx].focused_pane, pane2_id,
            "포커스와 무관한 pane 이 닫혔는데 포커스가 움직이면 안 된다"
        );
    }

    #[test]
    fn case4_workspace_close_reports_the_removed_index() {
        let mut engine = test_engine();
        let ws1_id = engine.next_ids.next_workspace();
        let pane1_id = engine.next_ids.next_pane();
        let tab1_id = engine.next_ids.next_tab();
        let sid1 = engine.next_ids.next_surface();
        insert_detached(&mut engine, sid1);
        engine.workspaces.insert(
            0,
            crate::model::Workspace::new_with_terminal_marker(
                ws1_id,
                "ws1".to_string(),
                pane1_id,
                tab1_id,
                sid1,
            ),
        );

        let ev = Core::apply_close_surface(&mut engine, sid1, false);
        match ev {
            CoreEvent::SurfaceClosed {
                workspace_purged, ..
            } => {
                assert_eq!(workspace_purged, Some((0, ws1_id)));
            }
            other => panic!("expected SurfaceClosed, got {other:?}"),
        }
    }

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
                workspace_purged,
                workspaces_now_empty,
                ..
            } => {
                assert!(closed);
                assert_eq!(cascade_level, CascadeLevel::Workspace);
                assert_eq!(workspace_purged.map(|(_, id)| id), Some(ws0_id));
                assert!(workspaces_now_empty);
            }
            other => panic!("expected SurfaceClosed, got {other:?}"),
        }
        assert!(engine.workspaces.is_empty());
    }

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
                workspace_purged,
                ..
            } => {
                assert!(closed);
                assert_eq!(cascade_level, CascadeLevel::Surface);
                assert_eq!(cleanup_targets, vec![(sid_a, None)]);
                assert!(closed_tab_ids.is_empty());
                assert!(closed_pane_ids.is_empty());
                assert_eq!(workspace_purged, None);
            }
            other => panic!("expected SurfaceClosed, got {other:?}"),
        }
    }
}
