//! 탭·페인을 다른 탭·페인 자리로 옮긴다(replace). 옮기는 Tab·Pane 객체와 Terminal은 그대로 두고
//! 덮어쓴 쪽의 후속 정리 대상을 반환한다. 명세: docs/features/surface-move/index.md.

use super::*;
use crate::core::engine_access::EngineMut;
use crate::core::impl_close::collect_close_targets;
use crate::core::intent::CascadeLevel;

/// source가 떠나 비게 된 구조. 이벤트의 source 쪽 필드로 옮긴다.
struct SourceDetached {
    cascade_level: CascadeLevel,
    closed_pane_ids: Vec<u32>,
    workspace_purged: Option<(usize, u32)>,
    workspaces_now_empty: bool,
}

fn container_move_noop() -> CoreEvent {
    CoreEvent::ContainerMoveApplied {
        replaced_tab: None,
        replaced_pane: None,
        moved: false,
        cleanup_targets: vec![],
        cascade_level: CascadeLevel::Tab,
        closed_tab_ids: vec![],
        closed_tabs_pane: None,
        closed_pane_ids: vec![],
        workspace_purged: None,
        workspaces_now_empty: false,
    }
}

/// source를 떼고 난 뒤 target을 잃은 경우. 이미 바뀐 구조 정보는 싣되 moved=false라 정리는 실행되지 않는다.
fn container_move_failed(detached: SourceDetached) -> CoreEvent {
    CoreEvent::ContainerMoveApplied {
        replaced_tab: None,
        replaced_pane: None,
        moved: false,
        cleanup_targets: vec![],
        cascade_level: detached.cascade_level,
        closed_tab_ids: vec![],
        closed_tabs_pane: None,
        closed_pane_ids: detached.closed_pane_ids,
        workspace_purged: detached.workspace_purged,
        workspaces_now_empty: detached.workspaces_now_empty,
    }
}

impl Core {
    /// source 탭을 떼어 target 탭 자리에 넣고 target 탭을 닫힌 것으로 반환한다.
    /// Terminal store는 여기서 지우지 않는다. 성공하지 않아도 이동 대기 슬롯은 비운다.
    pub(super) fn apply_replace_tab_with_tab(
        engine: &mut EngineMut<'_>,
        source_tab_id: u32,
        target_tab_id: u32,
    ) -> CoreEvent {
        engine.pending_move = None;

        if source_tab_id == target_tab_id {
            return container_move_noop();
        }
        let Some(source_pane_id) = engine.find_pane_for_tab(source_tab_id) else {
            return container_move_noop();
        };
        if engine.find_pane_for_tab(target_tab_id).is_none() {
            return container_move_noop();
        }

        let Some((tab, detached)) =
            Self::detach_tab_for_move(engine, source_pane_id, source_tab_id)
        else {
            return container_move_noop();
        };

        // source 제거로 인덱스가 바뀌었을 수 있어 target을 ID로 다시 찾는다.
        let Some(target_pane_id) = engine.find_pane_for_tab(target_tab_id) else {
            tracing::error!(
                source_tab_id,
                target_tab_id,
                "move tab: target not found after detaching source"
            );
            return container_move_failed(detached);
        };
        let Some(pane) = engine.find_pane_by_id_mut(target_pane_id) else {
            return container_move_failed(detached);
        };
        let Some(target_idx) = pane.tabs.iter().position(|t| t.id == target_tab_id) else {
            return container_move_failed(detached);
        };
        // 같은 인덱스를 교체하므로 target이 활성 탭이었다면 옮긴 탭이 활성 탭이 된다.
        let replaced = std::mem::replace(&mut pane.tabs[target_idx], tab);
        let moved_focus = pane.tabs[target_idx].first_surface_id();

        let mut cleanup_targets = Vec::new();
        collect_close_targets(&replaced, &engine.as_ref(), &mut cleanup_targets);
        engine.mark_layout_dirty();
        if let Some(surface_id) = moved_focus {
            engine.refresh_tab_osc_title(surface_id);
        }

        CoreEvent::ContainerMoveApplied {
            replaced_tab: Some((target_tab_id, source_tab_id)),
            replaced_pane: None,
            moved: true,
            cleanup_targets,
            cascade_level: detached.cascade_level,
            closed_tab_ids: vec![target_tab_id],
            closed_tabs_pane: Some(target_pane_id),
            closed_pane_ids: detached.closed_pane_ids,
            workspace_purged: detached.workspace_purged,
            workspaces_now_empty: detached.workspaces_now_empty,
        }
    }

    /// source 페인을 떼어 target 페인 자리(분할 트리의 같은 위치·비율)에 넣고 target 페인을 닫힌 것으로 반환한다.
    /// Terminal store는 여기서 지우지 않는다. 성공하지 않아도 이동 대기 슬롯은 비운다.
    pub(super) fn apply_replace_pane_with_pane(
        engine: &mut EngineMut<'_>,
        source_pane_id: u32,
        target_pane_id: u32,
    ) -> CoreEvent {
        engine.pending_move = None;

        if source_pane_id == target_pane_id {
            return container_move_noop();
        }
        let Some(source_ws) = engine.find_workspace_index_for_pane(source_pane_id) else {
            return container_move_noop();
        };
        if engine
            .find_workspace_index_for_pane(target_pane_id)
            .is_none()
        {
            return container_move_noop();
        }

        let Some((pane, detached)) = Self::detach_pane_for_move(engine, source_ws, source_pane_id)
        else {
            return container_move_noop();
        };

        // source workspace가 사라졌으면 인덱스가 바뀌므로 target을 ID로 다시 찾는다.
        let Some(target_ws) = engine.find_workspace_index_for_pane(target_pane_id) else {
            tracing::error!(
                source_pane_id,
                target_pane_id,
                "move pane: target not found after detaching source"
            );
            return container_move_failed(detached);
        };
        let ws = &mut engine.workspaces[target_ws];
        let replaced = match ws.pane_layout_mut().replace_pane(target_pane_id, pane) {
            Ok(replaced) => replaced,
            Err(_) => {
                tracing::error!(
                    source_pane_id,
                    target_pane_id,
                    "move pane: failed to replace target pane"
                );
                return container_move_failed(detached);
            }
        };

        let mut cleanup_targets = Vec::new();
        for tab in &replaced.tabs {
            collect_close_targets(tab, &engine.as_ref(), &mut cleanup_targets);
        }
        let closed_tab_ids = replaced.tabs.iter().map(|t| t.id).collect();
        let mut closed_pane_ids = detached.closed_pane_ids;
        closed_pane_ids.push(target_pane_id);
        engine.mark_layout_dirty();

        CoreEvent::ContainerMoveApplied {
            replaced_tab: None,
            replaced_pane: Some((target_pane_id, source_pane_id)),
            moved: true,
            cleanup_targets,
            cascade_level: detached.cascade_level,
            closed_tab_ids,
            closed_tabs_pane: Some(target_pane_id),
            closed_pane_ids,
            workspace_purged: detached.workspace_purged,
            workspaces_now_empty: detached.workspaces_now_empty,
        }
    }

    /// source 페인을 떼어 반환한다. 다른 페인이 있으면 형제가 자리를 채우고, 유일 페인이면
    /// workspace를 제거한다. 떠난 페인 자신은 닫힌 목록에 넣지 않는다.
    fn detach_pane_for_move(
        engine: &mut crate::core::CoreState,
        ws_idx: usize,
        pane_id: u32,
    ) -> Option<(crate::model::Pane, SourceDetached)> {
        let panes_len = engine.workspaces[ws_idx].pane_layout().all_pane_ids().len();
        if panes_len > 1 {
            let pane = engine.workspaces[ws_idx].detach_pane(pane_id)?;
            engine.mark_layout_dirty();
            return Some((
                pane,
                SourceDetached {
                    cascade_level: CascadeLevel::Pane,
                    closed_pane_ids: vec![],
                    workspace_purged: None,
                    workspaces_now_empty: false,
                },
            ));
        }

        // 제거한 workspace에서 pane을 못 찾는 일이 없도록 제거 전에 확인한다.
        engine.workspaces[ws_idx].pane_layout().find_pane(pane_id)?;
        let workspace_id = engine.workspaces[ws_idx].id;
        let mut ws = engine.workspaces.remove(ws_idx);
        let empty = crate::model::Pane {
            id: 0,
            tabs: vec![],
        };
        let pane = std::mem::replace(ws.pane_layout_mut().find_pane_mut(pane_id)?, empty);
        let workspaces_now_empty = engine.workspaces.is_empty();
        engine.mark_layout_dirty();
        Some((
            pane,
            SourceDetached {
                cascade_level: CascadeLevel::Workspace,
                closed_pane_ids: vec![],
                workspace_purged: Some((ws_idx, workspace_id)),
                workspaces_now_empty,
            },
        ))
    }

    /// source 탭을 떼어 반환한다. 비게 된 pane·workspace는 지우되 Terminal store와
    /// scrollback은 유지하며 닫기 snapshot도 만들지 않는다. 떠난 탭 자신은 닫힌 목록에 넣지 않는다.
    fn detach_tab_for_move(
        engine: &mut crate::core::CoreState,
        pane_id: u32,
        tab_id: u32,
    ) -> Option<(crate::model::Tab, SourceDetached)> {
        let ws_idx = engine.find_workspace_index_for_pane(pane_id)?;
        let (tabs_len, panes_len, tab_idx) = {
            let ws = &engine.workspaces[ws_idx];
            let pane = ws.pane_layout().find_pane(pane_id)?;
            (
                pane.tabs.len(),
                ws.pane_layout().all_pane_ids().len(),
                pane.tabs.iter().position(|t| t.id == tab_id)?,
            )
        };

        let ws = &mut engine.workspaces[ws_idx];
        let tab = ws
            .pane_layout_mut()
            .find_pane_mut(pane_id)?
            .take_tab(tab_idx)?;

        if tabs_len > 1 {
            engine.mark_layout_dirty();
            return Some((
                tab,
                SourceDetached {
                    cascade_level: CascadeLevel::Tab,
                    closed_pane_ids: vec![],
                    workspace_purged: None,
                    workspaces_now_empty: false,
                },
            ));
        }

        if panes_len > 1 {
            if !ws.close_pane(pane_id) {
                // 닫지 못한 pane이 탭 없이 남지 않도록 되돌린다.
                if let Some(pane) = ws.pane_layout_mut().find_pane_mut(pane_id) {
                    pane.tabs.push(tab);
                }
                return None;
            }
            engine.mark_layout_dirty();
            return Some((
                tab,
                SourceDetached {
                    cascade_level: CascadeLevel::Pane,
                    closed_pane_ids: vec![pane_id],
                    workspace_purged: None,
                    workspaces_now_empty: false,
                },
            ));
        }

        let workspace_id = engine.workspaces[ws_idx].id;
        engine.workspaces.remove(ws_idx);
        let workspaces_now_empty = engine.workspaces.is_empty();
        engine.mark_layout_dirty();
        Some((
            tab,
            SourceDetached {
                cascade_level: CascadeLevel::Workspace,
                closed_pane_ids: vec![pane_id],
                workspace_purged: Some((ws_idx, workspace_id)),
                workspaces_now_empty,
            },
        ))
    }
}

#[cfg(test)]
mod move_container_tests {
    use super::*;
    use crate::core::intent::DomainIntent;
    use crate::core::state::PendingMove;
    use crate::model::SplitDirection;

    fn test_engine() -> crate::runtime::engine_session::EngineSession {
        let waker: tasty_terminal::Waker = std::sync::Arc::new(|| {});
        crate::runtime::engine_session::EngineSession::new(80, 24, waker).expect("engine")
    }

    /// 첫 workspace의 첫 pane과 그 첫 탭·surface. surface에는 detached Terminal을 붙인다.
    fn first_pane(engine: &mut EngineMut<'_>) -> (u32, u32, u32) {
        let a = engine.workspaces[0].all_surface_ids()[0];
        engine
            .runtime
            .terminals
            .insert(a, tasty_terminal::Terminal::new_detached(80, 24), None);
        let (_, pane_id) = engine.find_workspace_index_for_surface(a).unwrap();
        let tab_id = engine.find_pane_by_id(pane_id).unwrap().tabs[0].id;
        (pane_id, tab_id, a)
    }

    /// pane에 detached Terminal을 가진 탭을 하나 더 붙인다.
    fn add_tab(engine: &mut EngineMut<'_>, pane_id: u32) -> (u32, u32) {
        let tab_id = engine.next_ids.next_tab();
        let sid = engine.next_ids.next_surface();
        engine
            .runtime
            .terminals
            .insert(sid, tasty_terminal::Terminal::new_detached(80, 24), None);
        engine
            .find_pane_by_id_mut(pane_id)
            .unwrap()
            .add_terminal_marker_tab(tab_id, sid);
        (tab_id, sid)
    }

    /// 첫 workspace의 pane을 나눠 새 pane(탭 하나)을 만든다.
    fn split_new_pane(engine: &mut EngineMut<'_>, pane_id: u32) -> (u32, u32, u32) {
        let new_pane_id = engine.next_ids.next_pane();
        let tab_id = engine.next_ids.next_tab();
        let sid = engine.next_ids.next_surface();
        engine
            .runtime
            .terminals
            .insert(sid, tasty_terminal::Terminal::new_detached(80, 24), None);
        let pane = crate::model::Pane::new_with_terminal_marker(new_pane_id, tab_id, sid);
        let ws_idx = engine.find_workspace_index_for_pane(pane_id).unwrap();
        assert!(
            engine.workspaces[ws_idx]
                .pane_layout_mut()
                .split_pane_in_place(pane_id, SplitDirection::Horizontal, pane)
                .is_none()
        );
        (new_pane_id, tab_id, sid)
    }

    /// 새 workspace(pane 하나, 탭 하나)를 붙인다.
    fn push_workspace(engine: &mut EngineMut<'_>) -> (u32, u32, u32) {
        let ws_id = engine.next_ids.next_workspace();
        let pane_id = engine.next_ids.next_pane();
        let tab_id = engine.next_ids.next_tab();
        let sid = engine.next_ids.next_surface();
        engine
            .runtime
            .terminals
            .insert(sid, tasty_terminal::Terminal::new_detached(80, 24), None);
        engine
            .workspaces
            .push(crate::model::Workspace::new_with_terminal_marker(
                ws_id,
                "ws1".to_string(),
                pane_id,
                tab_id,
                sid,
            ));
        (pane_id, tab_id, sid)
    }

    #[test]
    fn move_tab_replaces_target_tab_and_keeps_source_terminal() {
        let mut engine_session = test_engine();
        let mut engine = engine_session.borrow_mut();
        let (pane, tab_a, a) = first_pane(&mut engine);
        let (tab_b, b) = add_tab(&mut engine, pane);
        engine.pending_move = Some(PendingMove::Tab(tab_a));

        let ev = Core::apply_replace_tab_with_tab(&mut engine, tab_a, tab_b);
        let CoreEvent::ContainerMoveApplied {
            moved,
            cleanup_targets,
            cascade_level,
            closed_tab_ids,
            closed_tabs_pane,
            closed_pane_ids,
            workspace_purged,
            ..
        } = ev
        else {
            panic!("unexpected event: {ev:?}");
        };
        assert!(moved);
        assert_eq!(cleanup_targets, vec![(b, None)]);
        assert!(matches!(cascade_level, CascadeLevel::Tab));
        assert_eq!(closed_tab_ids, vec![tab_b]);
        assert_eq!(closed_tabs_pane, Some(pane));
        assert!(closed_pane_ids.is_empty());
        assert!(workspace_purged.is_none());

        let p = engine.find_pane_by_id(pane).unwrap();
        assert_eq!(p.tabs.iter().map(|t| t.id).collect::<Vec<_>>(), vec![tab_a]);
        assert!(p.tabs[0].contains_surface(a));
        assert!(
            engine.runtime.terminals.contains(a),
            "source terminal must survive"
        );
        assert!(
            engine.runtime.terminals.contains(b),
            "target store 제거는 호출자의 후속 처리다"
        );
        assert!(engine.pending_move.is_none());
    }

    #[test]
    fn move_tab_to_later_index_in_same_pane_finds_target_by_id() {
        let mut engine_session = test_engine();
        let mut engine = engine_session.borrow_mut();
        let (pane, tab_a, _a) = first_pane(&mut engine);
        let (tab_b, _b) = add_tab(&mut engine, pane);
        let (tab_c, _c) = add_tab(&mut engine, pane);
        // source(0)를 떼면 target C 의 인덱스가 2 → 1 로 줄어든다.
        let ev = Core::apply_replace_tab_with_tab(&mut engine, tab_a, tab_c);
        assert!(matches!(
            ev,
            CoreEvent::ContainerMoveApplied { moved: true, .. }
        ));
        let p = engine.find_pane_by_id(pane).unwrap();
        assert_eq!(
            p.tabs.iter().map(|t| t.id).collect::<Vec<_>>(),
            vec![tab_b, tab_a]
        );
    }

    #[test]
    fn move_tab_into_active_target_makes_it_active() {
        let mut engine_session = test_engine();
        let mut engine = engine_session.borrow_mut();
        let (pane, tab_a, _a) = first_pane(&mut engine);
        let (other_pane, tab_b, _b) = split_new_pane(&mut engine, pane);
        let (tab_c, _c) = add_tab(&mut engine, other_pane);
        let mut navigation = crate::state::navigation::NavigationState::default();
        navigation.goto_tab(engine.find_pane_by_id(other_pane).unwrap(), 0);
        // P1 에 탭을 하나 더 두어 source pane 이 남게 한다.
        add_tab(&mut engine, pane);

        let event = Core::apply_replace_tab_with_tab(&mut engine, tab_a, tab_b);
        navigation.apply_result(&engine.workspaces, &event);
        let p = engine.find_pane_by_id(other_pane).unwrap();
        assert_eq!(
            p.tabs.iter().map(|t| t.id).collect::<Vec<_>>(),
            vec![tab_a, tab_c]
        );
        assert_eq!(navigation.tab_index(p), 0);
    }

    #[test]
    fn move_last_tab_cascades_source_pane() {
        let mut engine_session = test_engine();
        let mut engine = engine_session.borrow_mut();
        let (p1, tab_a, a) = first_pane(&mut engine);
        let (p2, tab_b, b) = split_new_pane(&mut engine, p1);

        let ev = Core::apply_replace_tab_with_tab(&mut engine, tab_a, tab_b);
        let CoreEvent::ContainerMoveApplied {
            moved,
            cleanup_targets,
            cascade_level,
            closed_tab_ids,
            closed_tabs_pane,
            closed_pane_ids,
            workspace_purged,
            ..
        } = ev
        else {
            panic!("unexpected event: {ev:?}");
        };
        assert!(moved);
        assert!(matches!(cascade_level, CascadeLevel::Pane));
        assert_eq!(closed_pane_ids, vec![p1]);
        assert_eq!(closed_tab_ids, vec![tab_b]);
        // 닫힌 탭 B 는 source pane 이 아니라 원래 있던 P2 소속으로 알린다.
        assert_eq!(closed_tabs_pane, Some(p2));
        assert_eq!(cleanup_targets, vec![(b, None)]);
        assert!(workspace_purged.is_none());
        assert!(engine.find_pane_by_id(p1).is_none());
        let p = engine.find_pane_by_id(p2).unwrap();
        assert_eq!(p.tabs.iter().map(|t| t.id).collect::<Vec<_>>(), vec![tab_a]);
        assert!(engine.runtime.terminals.contains(a));
    }

    #[test]
    fn move_only_tab_of_workspace_purges_source_workspace() {
        let mut engine_session = test_engine();
        let mut engine = engine_session.borrow_mut();
        let (p0, tab_a, a) = first_pane(&mut engine);
        let ws0_id = engine.workspaces[0].id;
        let (q, tab_q, q_sid) = push_workspace(&mut engine);

        let ev = Core::apply_replace_tab_with_tab(&mut engine, tab_a, tab_q);
        let CoreEvent::ContainerMoveApplied {
            moved,
            cleanup_targets,
            cascade_level,
            closed_pane_ids,
            workspace_purged,
            workspaces_now_empty,
            ..
        } = ev
        else {
            panic!("unexpected event: {ev:?}");
        };
        assert!(moved);
        assert!(matches!(cascade_level, CascadeLevel::Workspace));
        assert_eq!(closed_pane_ids, vec![p0]);
        assert_eq!(workspace_purged, Some((0, ws0_id)));
        assert!(!workspaces_now_empty);
        assert_eq!(cleanup_targets, vec![(q_sid, None)]);
        assert_eq!(engine.workspaces.len(), 1);
        let p = engine.find_pane_by_id(q).unwrap();
        assert_eq!(p.tabs.iter().map(|t| t.id).collect::<Vec<_>>(), vec![tab_a]);
        assert!(engine.runtime.terminals.contains(a));
    }

    #[test]
    fn move_tab_self_ref_is_noop_and_clears_slot() {
        let mut engine_session = test_engine();
        let mut engine = engine_session.borrow_mut();
        let (pane, tab_a, _a) = first_pane(&mut engine);
        engine.pending_move = Some(PendingMove::Tab(tab_a));
        let ev = Core::apply_replace_tab_with_tab(&mut engine, tab_a, tab_a);
        assert!(matches!(
            ev,
            CoreEvent::ContainerMoveApplied { moved: false, .. }
        ));
        assert!(engine.pending_move.is_none());
        assert_eq!(engine.find_pane_by_id(pane).unwrap().tabs.len(), 1);
    }

    #[test]
    fn move_tab_missing_target_is_noop_and_keeps_source() {
        let mut engine_session = test_engine();
        let mut engine = engine_session.borrow_mut();
        let (pane, tab_a, _a) = first_pane(&mut engine);
        engine.pending_move = Some(PendingMove::Tab(tab_a));
        let ev = Core::apply_replace_tab_with_tab(&mut engine, tab_a, 999_999);
        assert!(matches!(
            ev,
            CoreEvent::ContainerMoveApplied { moved: false, .. }
        ));
        assert!(engine.pending_move.is_none());
        assert_eq!(engine.find_pane_by_id(pane).unwrap().tabs[0].id, tab_a);
    }

    #[test]
    fn move_tab_into_mirror_workspace_is_blocked() {
        let mut engine_session = test_engine();
        let mut engine = engine_session.borrow_mut();
        let (_pane, tab_a, _a) = first_pane(&mut engine);
        let (_q, tab_q, _q_sid) = push_workspace(&mut engine);
        let intent = DomainIntent::ReplaceTabWithTab {
            source_tab_id: tab_a,
            target_tab_id: tab_q,
        };
        assert_eq!(engine.mirror_workspace_index_for_structural(&intent), None);
        engine.workspaces[1].mirror = true;
        assert_eq!(
            engine.mirror_workspace_index_for_structural(&intent),
            Some(1)
        );
        engine.workspaces[1].mirror = false;
        engine.workspaces[0].mirror = true;
        assert_eq!(
            engine.mirror_workspace_index_for_structural(&intent),
            Some(0)
        );
    }

    #[test]
    fn moved_tab_is_not_recorded_in_closed_history() {
        let mut engine_session = test_engine();
        let mut engine = engine_session.borrow_mut();
        let (pane, tab_a, _a) = first_pane(&mut engine);
        let (tab_b, _b) = add_tab(&mut engine, pane);
        assert!(engine.closed_items.is_empty());
        Core::apply_replace_tab_with_tab(&mut engine, tab_a, tab_b);
        assert!(engine.closed_items.is_empty());
    }

    /// 호스트 이벤트 대상 탭 ID들의 (탭, pane) 쌍을 모은다.
    #[cfg(feature = "gui")]
    fn run_move_and_collect_host_events(
        source_tab: u32,
        target_tab: u32,
        state: &mut crate::state::RequestContext,
        engine: &mut EngineMut<'_>,
    ) -> Vec<crate::state::PendingHostEvent> {
        use crate::app::structural_cascade::{SurfaceCloseCascade, cascade_surface_closed};
        let mut core = crate::ipc::handler::cli_entry_tests::test_core();
        state.detect_tab_lifecycle(engine);
        // 이동 전 준비 과정의 알림은 검사 대상이 아니므로 버린다.
        state.take_pending_host_events();
        let ev = Core::apply_replace_tab_with_tab(engine, source_tab, target_tab);
        let c = SurfaceCloseCascade::from_container_move_applied(ev, true).expect("moved");
        cascade_surface_closed(&mut core, state, engine, c);
        state.detect_tab_lifecycle(engine);
        state.take_pending_host_events()
    }

    /// source pane 이 사라져도 교체된 B 의 tab.closed 는 B 가 있던 pane 으로 나가고,
    /// 옮긴 A 는 tab.moved 한 번만 낸다.
    #[cfg(feature = "gui")]
    #[test]
    fn move_tab_host_events_keep_closed_tab_in_its_own_pane() {
        use crate::state::PendingHostEvent as E;
        let (mut state, mut engine_session) = crate::state::tests::test_state();
        let mut engine = engine_session.borrow_mut();
        let (p1, tab_a, _a) = first_pane(&mut engine);
        let (p2, tab_b, _b) = split_new_pane(&mut engine, p1);

        let events = run_move_and_collect_host_events(tab_a, tab_b, &mut state, &mut engine);

        let moved: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                E::TabMoved {
                    tab_id,
                    from_pane,
                    to_pane,
                } => Some((*tab_id, *from_pane, *to_pane)),
                _ => None,
            })
            .collect();
        assert_eq!(moved, vec![(tab_a, p1, p2)]);
        let closed: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                E::TabClosed { tab_id, pane_id } => Some((*tab_id, *pane_id)),
                _ => None,
            })
            .collect();
        assert_eq!(closed, vec![(tab_b, p2)]);
        assert!(
            events
                .iter()
                .any(|e| matches!(e, E::PaneClosed { pane_id } if *pane_id == p1))
        );
    }

    /// 같은 pane 안의 교체는 tab.moved 없이 B 의 tab.closed 만 낸다.
    #[cfg(feature = "gui")]
    #[test]
    fn move_tab_within_pane_emits_only_the_target_close() {
        use crate::state::PendingHostEvent as E;
        let (mut state, mut engine_session) = crate::state::tests::test_state();
        let mut engine = engine_session.borrow_mut();
        let (pane, tab_a, _a) = first_pane(&mut engine);
        let (tab_b, _b) = add_tab(&mut engine, pane);

        let events = run_move_and_collect_host_events(tab_a, tab_b, &mut state, &mut engine);

        assert!(!events.iter().any(|e| matches!(e, E::TabMoved { .. })));
        let closed: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                E::TabClosed { tab_id, pane_id } => Some((*tab_id, *pane_id)),
                _ => None,
            })
            .collect();
        assert_eq!(closed, vec![(tab_b, pane)]);
    }

    #[test]
    fn move_pane_replaces_target_and_keeps_source_terminals() {
        let mut engine_session = test_engine();
        let mut engine = engine_session.borrow_mut();
        let (p1, tab_a, a) = first_pane(&mut engine);
        let (p2, _tab_p2, _p2_sid) = split_new_pane(&mut engine, p1);
        let (q, tab_q, q_sid) = push_workspace(&mut engine);
        let (tab_q2, q2_sid) = add_tab(&mut engine, q);
        let mut navigation = crate::state::navigation::NavigationState::default();
        navigation.select_pane(&engine.workspaces[1], q);
        engine.pending_move = Some(PendingMove::Pane(p1));

        let ev = Core::apply_replace_pane_with_pane(&mut engine, p1, q);
        navigation.apply_result(&engine.workspaces, &ev);
        let CoreEvent::ContainerMoveApplied {
            moved,
            cleanup_targets,
            cascade_level,
            closed_tab_ids,
            closed_tabs_pane,
            closed_pane_ids,
            workspace_purged,
            ..
        } = ev
        else {
            panic!("unexpected event: {ev:?}");
        };
        assert!(moved);
        assert!(matches!(cascade_level, CascadeLevel::Pane));
        assert_eq!(cleanup_targets, vec![(q_sid, None), (q2_sid, None)]);
        assert_eq!(closed_tab_ids, vec![tab_q, tab_q2]);
        assert_eq!(closed_tabs_pane, Some(q));
        assert_eq!(closed_pane_ids, vec![q]);
        assert!(workspace_purged.is_none());

        assert_eq!(engine.workspaces[0].pane_layout().all_pane_ids(), vec![p2]);
        assert_eq!(engine.workspaces[1].pane_layout().all_pane_ids(), vec![p1]);
        assert_eq!(navigation.pane_id(&engine.workspaces[1]).unwrap(), p1);
        let moved_pane = engine.find_pane_by_id(p1).unwrap();
        assert_eq!(moved_pane.tabs[0].id, tab_a);
        assert!(
            engine.runtime.terminals.contains(a),
            "source terminal must survive"
        );
        assert!(engine.pending_move.is_none());
    }

    #[test]
    fn move_pane_inherits_target_split_position() {
        let mut engine_session = test_engine();
        let mut engine = engine_session.borrow_mut();
        let (p1, _tab_a, _a) = first_pane(&mut engine);
        let (_q, _tab_q, _q_sid) = push_workspace(&mut engine);
        let q = engine.workspaces[1].pane_layout().all_pane_ids()[0];
        let new_pane_id = engine.next_ids.next_pane();
        let tab_id = engine.next_ids.next_tab();
        let sid = engine.next_ids.next_surface();
        let r = crate::model::Pane::new_with_terminal_marker(new_pane_id, tab_id, sid);
        assert!(
            engine.workspaces[1]
                .pane_layout_mut()
                .split_pane_in_place(q, SplitDirection::Vertical, r)
                .is_none()
        );
        if let crate::model::PaneNode::Split { ratio, .. } = engine.workspaces[1].pane_layout_mut()
        {
            *ratio = 0.3;
        }
        let (p_extra, _, _) = split_new_pane(&mut engine, p1);

        Core::apply_replace_pane_with_pane(&mut engine, p1, new_pane_id);
        assert_eq!(
            engine.workspaces[0].pane_layout().all_pane_ids(),
            vec![p_extra]
        );
        assert_eq!(
            engine.workspaces[1].pane_layout().all_pane_ids(),
            vec![q, p1]
        );
        assert!(matches!(
            engine.workspaces[1].pane_layout(),
            crate::model::PaneNode::Split { ratio, .. } if *ratio == 0.3
        ));
    }

    #[test]
    fn move_only_pane_purges_source_workspace() {
        let mut engine_session = test_engine();
        let mut engine = engine_session.borrow_mut();
        let (p0, _tab_a, a) = first_pane(&mut engine);
        let ws0_id = engine.workspaces[0].id;
        let (q, _tab_q, q_sid) = push_workspace(&mut engine);

        let ev = Core::apply_replace_pane_with_pane(&mut engine, p0, q);
        let CoreEvent::ContainerMoveApplied {
            moved,
            cleanup_targets,
            cascade_level,
            closed_pane_ids,
            workspace_purged,
            workspaces_now_empty,
            ..
        } = ev
        else {
            panic!("unexpected event: {ev:?}");
        };
        assert!(moved);
        assert!(matches!(cascade_level, CascadeLevel::Workspace));
        assert_eq!(workspace_purged, Some((0, ws0_id)));
        assert!(!workspaces_now_empty);
        assert_eq!(closed_pane_ids, vec![q], "source pane 은 닫힌 목록에 없다");
        assert_eq!(cleanup_targets, vec![(q_sid, None)]);
        assert_eq!(engine.workspaces.len(), 1);
        assert_eq!(engine.workspaces[0].pane_layout().all_pane_ids(), vec![p0]);
        assert!(engine.runtime.terminals.contains(a));
    }

    #[test]
    fn move_pane_self_ref_is_noop_and_clears_slot() {
        let mut engine_session = test_engine();
        let mut engine = engine_session.borrow_mut();
        let (p1, _tab_a, _a) = first_pane(&mut engine);
        engine.pending_move = Some(PendingMove::Pane(p1));
        let ev = Core::apply_replace_pane_with_pane(&mut engine, p1, p1);
        assert!(matches!(
            ev,
            CoreEvent::ContainerMoveApplied { moved: false, .. }
        ));
        assert!(engine.pending_move.is_none());
        assert_eq!(engine.workspaces[0].pane_layout().all_pane_ids(), vec![p1]);
    }

    #[test]
    fn move_pane_missing_target_is_noop_and_keeps_source() {
        let mut engine_session = test_engine();
        let mut engine = engine_session.borrow_mut();
        let (p1, _tab_a, _a) = first_pane(&mut engine);
        let (p2, _, _) = split_new_pane(&mut engine, p1);
        engine.pending_move = Some(PendingMove::Pane(p1));
        let ev = Core::apply_replace_pane_with_pane(&mut engine, p1, 999_999);
        assert!(matches!(
            ev,
            CoreEvent::ContainerMoveApplied { moved: false, .. }
        ));
        assert!(engine.pending_move.is_none());
        assert_eq!(
            engine.workspaces[0].pane_layout().all_pane_ids(),
            vec![p1, p2]
        );
    }

    #[test]
    fn move_pane_into_mirror_workspace_is_blocked() {
        let mut engine_session = test_engine();
        let mut engine = engine_session.borrow_mut();
        let (p1, _tab_a, _a) = first_pane(&mut engine);
        let (q, _tab_q, _q_sid) = push_workspace(&mut engine);
        let intent = DomainIntent::ReplacePaneWithPane {
            source_pane_id: p1,
            target_pane_id: q,
        };
        assert_eq!(engine.mirror_workspace_index_for_structural(&intent), None);
        engine.workspaces[1].mirror = true;
        assert_eq!(
            engine.mirror_workspace_index_for_structural(&intent),
            Some(1)
        );
        engine.workspaces[1].mirror = false;
        engine.workspaces[0].mirror = true;
        assert_eq!(
            engine.mirror_workspace_index_for_structural(&intent),
            Some(0)
        );
    }

    #[test]
    fn moved_pane_target_is_not_recorded_in_closed_history() {
        let mut engine_session = test_engine();
        let mut engine = engine_session.borrow_mut();
        let (p1, _tab_a, _a) = first_pane(&mut engine);
        let (q, _tab_q, _q_sid) = push_workspace(&mut engine);
        Core::apply_replace_pane_with_pane(&mut engine, p1, q);
        assert!(engine.closed_items.is_empty());
    }

    /// 페인 이동의 호스트 이벤트: 교체된 Q 의 탭은 Q 소속으로 닫히고 Q 의 pane.closed 가 난다.
    /// 옮긴 페인의 탭은 pane ID 가 같아 tab.moved 를 내지 않는다.
    #[cfg(feature = "gui")]
    #[test]
    fn move_pane_host_events_close_only_the_target() {
        use crate::app::structural_cascade::{SurfaceCloseCascade, cascade_surface_closed};
        use crate::state::PendingHostEvent as E;
        let (mut state, mut engine_session) = crate::state::tests::test_state();
        let mut engine = engine_session.borrow_mut();
        let (p1, _tab_a, _a) = first_pane(&mut engine);
        split_new_pane(&mut engine, p1);
        let (q, tab_q, _q_sid) = push_workspace(&mut engine);
        let mut core = crate::ipc::handler::cli_entry_tests::test_core();
        state.detect_tab_lifecycle(&engine);
        // 이동 전 준비 과정의 알림은 검사 대상이 아니므로 버린다.
        state.take_pending_host_events();

        let ev = Core::apply_replace_pane_with_pane(&mut engine, p1, q);
        let c = SurfaceCloseCascade::from_container_move_applied(ev, true).expect("moved");
        cascade_surface_closed(&mut core, &mut state, &mut engine, c);
        state.detect_tab_lifecycle(&engine);
        let events = state.take_pending_host_events();

        assert!(!events.iter().any(|e| matches!(e, E::TabMoved { .. })));
        let closed: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                E::TabClosed { tab_id, pane_id } => Some((*tab_id, *pane_id)),
                _ => None,
            })
            .collect();
        assert_eq!(closed, vec![(tab_q, q)]);
        let closed_panes: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                E::PaneClosed { pane_id } => Some(*pane_id),
                _ => None,
            })
            .collect();
        assert_eq!(closed_panes, vec![q]);
    }
}
