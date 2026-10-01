//! surface·tab·pane·workspace를 닫고 호출자에게 후속 정리 대상을 반환한다.

use super::*;
use crate::runtime::engine_access::{EngineMut, EngineRef};

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
    engine: &EngineRef<'_>,
    out: &mut Vec<(u32, Option<String>)>,
) {
    tab.for_each_surface(&mut |s| {
        if let Some(ts) = s.as_any().downcast_ref::<crate::model::TerminalSurface>() {
            out.push((
                ts.id,
                engine
                    .runtime
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
    let ws = engine
        .workspace_at(ws_idx)
        .expect("workspace index is valid");
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

/// workspace 닫기 계측 로그의 경로 구분이다.
/// Cascade는 AppServices::apply 뒤 cascade_surface_closed가 전체 시간을 기록하도록 시작 시각을 맡긴다.
/// Inline은 호출한 창 경로가 전체 시간을 직접 기록한다.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum CloseTracePath {
    Cascade,
    Inline,
}

impl CloseTracePath {
    fn label(self) -> &'static str {
        match self {
            Self::Cascade => "cascade",
            Self::Inline => "inline",
        }
    }
}

pub(crate) fn surface_close_not_found(surface_id: u32) -> CoreEvent {
    CoreEvent::SurfaceClosed {
        surface_id,
        closed: false,
        cascade_level: crate::app::command::CascadeLevel::Surface,
        cleanup_targets: vec![],
        closed_tab_ids: vec![],
        closed_pane_ids: vec![],
        workspace_purged: None,
        workspaces_now_empty: false,
    }
}

impl AppServices {
    pub(super) fn apply_close_pane(engine: &mut EngineMut<'_>, pane_id: u32) -> CoreEvent {
        Self::close_pane_recording(engine, pane_id, None)
    }

    /// pane을 닫는다. save_snapshot이면 제거 전에 분할 위치를 포함한 복원 기록을 남긴다.
    /// 사용자 닫기만 기록한다. 기록 여부는 origin을 아는 진입점이 정한다.
    pub(crate) fn close_pane_recording(
        engine: &mut EngineMut<'_>,
        pane_id: u32,
        presentation: Option<&dyn crate::model::StructurePresentation>,
    ) -> CoreEvent {
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

        // 제거 후에는 부모 split 정보를 잃으므로 트리를 바꾸기 전에 복원 사본을 만든다.
        if let Some(presentation) = presentation
            && let Some(item) = engine.capture_closed_pane(pane_id, presentation)
        {
            engine.push_closed_item(item);
        }

        let mut targets: Vec<(u32, Option<String>)> = Vec::new();
        if let Some(pane) = engine
            .workspace_at(ws_idx)
            .expect("workspace index is valid")
            .pane_layout()
            .find_pane(pane_id)
        {
            for tab in &pane.tabs {
                collect_close_targets(tab, &engine.as_ref(), &mut targets);
            }
        }

        let ws = engine
            .workspace_at_mut(ws_idx)
            .expect("workspace index is valid");
        let removed = ws.close_pane(pane_id);
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
        engine: &mut EngineMut<'_>,
        surface_id: u32,
        presentation: Option<&dyn crate::model::StructurePresentation>,
    ) -> CoreEvent {
        Self::close_surface_recording(engine, surface_id, presentation, CloseTracePath::Cascade)
    }

    /// apply_close_surface와 같은 닫기다. 창 경로는 계측 경로를 Inline으로 넘겨 전체 시간을 직접 기록한다.
    /// save_snapshot은 사용자 닫기에서만 true다. 기록 여부는 origin을 아는 진입점이 정한다.
    pub(crate) fn close_surface_recording(
        engine: &mut EngineMut<'_>,
        surface_id: u32,
        presentation: Option<&dyn crate::model::StructurePresentation>,
        trace: CloseTracePath,
    ) -> CoreEvent {
        let loc = match locate_surface_in_pane(engine, surface_id) {
            Some(l) => l,
            None => return surface_close_not_found(surface_id),
        };
        if !loc.surface_is_sole_in_tab && loc.can_close_surface_in_group {
            return Self::close_case_split(engine, &loc, surface_id, presentation)
                .unwrap_or_else(|| surface_close_not_found(surface_id));
        }
        if let Some(ev) = Self::close_case_tab(engine, &loc, surface_id, presentation) {
            return ev;
        }
        if let Some(ev) = Self::close_case_pane(engine, &loc, surface_id, presentation) {
            return ev;
        }
        Self::close_case_workspace(engine, &loc, surface_id, presentation, trace)
    }

    fn close_case_split(
        engine: &mut EngineMut<'_>,
        loc: &SurfaceCloseLocation,
        surface_id: u32,
        presentation: Option<&dyn crate::model::StructurePresentation>,
    ) -> Option<CoreEvent> {
        use crate::app::command::CascadeLevel;
        if let Some(presentation) = presentation {
            let tab_name_opt = {
                let ws = engine
                    .workspace_at(loc.ws_idx)
                    .expect("workspace index is valid");
                let pane = ws.pane_layout().find_pane(loc.pane_id).unwrap();
                let tab = &pane.tabs[loc.tab_idx];
                if terminal_surface_in_tab(tab, surface_id).is_some() {
                    Some(tab.display_name(presentation.surface_id(tab)))
                } else {
                    None
                }
            };
            if let Some(tab_name) = tab_name_opt {
                let snapshot = crate::model::closed_item::ClosedSurface::from_capture(
                    surface_id,
                    engine.runtime.terminals.closed_capture(surface_id),
                );
                engine.push_closed_item(crate::model::ClosedItem::Surface {
                    surface: snapshot,
                    tab_name,
                });
            }
        }
        let persist_id = engine
            .runtime
            .terminals
            .scrollback_persist_id(surface_id)
            .map(str::to_string);
        let ws = engine
            .workspace_at_mut(loc.ws_idx)
            .expect("workspace index is valid");
        let pane = ws.pane_layout_mut().find_pane_mut(loc.pane_id).unwrap();
        let tab = &mut pane.tabs[loc.tab_idx];
        let closed = tab.close_surface(surface_id);
        // 닫힌 surface의 제목이 남지 않도록 새로 선택된 surface의 제목을 반영한다.
        let new_focused = tab.first_surface_id();
        if closed {
            engine.mark_layout_dirty();
            if let Some(surface_id) = new_focused {
                engine.refresh_tab_osc_title(surface_id);
            }
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
        engine: &mut EngineMut<'_>,
        loc: &SurfaceCloseLocation,
        surface_id: u32,
        presentation: Option<&dyn crate::model::StructurePresentation>,
    ) -> Option<CoreEvent> {
        use crate::app::command::CascadeLevel;
        if let Some(presentation) = presentation {
            let ws = engine
                .workspace_at(loc.ws_idx)
                .expect("workspace index is valid");
            let pane = ws.pane_layout().find_pane(loc.pane_id).unwrap();
            if pane.tabs.len() > 1 {
                let snapshot_opt = {
                    let mut snap_fn =
                        crate::runtime::surface_registry::snapshot_fn_for(&engine.runtime.surface_registry,&engine.runtime.surfaces);
                    let terminals = &engine.runtime.terminals;
                    crate::model::closed_item::ClosedTab::from_tab(
                        &pane.tabs[loc.tab_idx],
                        &mut snap_fn,
                        &|id| terminals.closed_capture(id),
                        presentation,
                    )
                };
                if let Some(snapshot) = snapshot_opt {
                    engine.push_closed_item(crate::model::ClosedItem::Tab(snapshot));
                }
            }
        }
        let mut targets: Vec<(u32, Option<String>)> = Vec::new();
        {
            let ws = engine
                .workspace_at(loc.ws_idx)
                .expect("workspace index is valid");
            let pane = ws.pane_layout().find_pane(loc.pane_id).unwrap();
            if pane.tabs.len() > 1 {
                collect_close_targets(&pane.tabs[loc.tab_idx], &engine.as_ref(), &mut targets);
            }
        }
        let ws = engine
            .workspace_at_mut(loc.ws_idx)
            .expect("workspace index is valid");
        let pane = ws.pane_layout_mut().find_pane_mut(loc.pane_id).unwrap();
        if pane.tabs.len() > 1 {
            let closed_tab_id = pane.tabs[loc.tab_idx].id;
            pane.remove_tab(loc.tab_idx);
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
        engine: &mut EngineMut<'_>,
        loc: &SurfaceCloseLocation,
        surface_id: u32,
        presentation: Option<&dyn crate::model::StructurePresentation>,
    ) -> Option<CoreEvent> {
        use crate::app::command::CascadeLevel;
        // pane 제거가 부모 split을 없애므로 복원할 분할 정보는 제거 전에 캡처한다.
        if let Some(presentation) = presentation {
            let ws = engine
                .workspace_at(loc.ws_idx)
                .expect("workspace index is valid");
            if ws.pane_layout().all_pane_ids().len() > 1
                && let Some(pane) = ws.pane_layout().find_pane(loc.pane_id)
                && let Some((direction, ratio, was_first, sibling_pane_id)) =
                    ws.pane_layout().locate_split_context(loc.pane_id)
            {
                let snapshot = {
                    let mut snap_fn =
                        crate::runtime::surface_registry::snapshot_fn_for(&engine.runtime.surface_registry,&engine.runtime.surfaces);
                    let terminals = &engine.runtime.terminals;
                    crate::model::ClosedItem::from_pane(
                        pane,
                        sibling_pane_id,
                        direction,
                        ratio,
                        was_first,
                        &mut snap_fn,
                        &|id| terminals.closed_capture(id),
                        presentation,
                    )
                };
                engine.push_closed_item(snapshot);
            }
        }
        let mut targets: Vec<(u32, Option<String>)> = Vec::new();
        let mut closed_tab_ids: Vec<u32> = Vec::new();
        {
            let ws = engine
                .workspace_at(loc.ws_idx)
                .expect("workspace index is valid");
            if ws.pane_layout().all_pane_ids().len() > 1
                && let Some(pane) = ws.pane_layout().find_pane(loc.pane_id)
            {
                for tab in &pane.tabs {
                    collect_close_targets(tab, &engine.as_ref(), &mut targets);
                    closed_tab_ids.push(tab.id);
                }
            }
        }
        let ws = engine
            .workspace_at_mut(loc.ws_idx)
            .expect("workspace index is valid");
        if ws.pane_layout().all_pane_ids().len() > 1 {
            ws.close_pane(loc.pane_id);
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
        engine: &mut EngineMut<'_>,
        loc: &SurfaceCloseLocation,
        surface_id: u32,
        presentation: Option<&dyn crate::model::StructurePresentation>,
        trace: CloseTracePath,
    ) -> CoreEvent {
        use crate::close_trace;
        use crate::app::command::CascadeLevel;
        use std::time::Instant;

        let path = trace.label();
        // 실제 자원 정리가 이 함수 뒤에 이어지므로 전체 측정 시작 시각을 넘긴다.
        if trace == CloseTracePath::Cascade {
            crate::close_trace::arm_cascade(Instant::now(), presentation.is_some());
        }
        if let Some(presentation) = presentation {
            let t = Instant::now();
            let item = {
                let mut snap_fn =
                    crate::runtime::surface_registry::snapshot_fn_for(&engine.runtime.surface_registry,&engine.runtime.surfaces);
                let ws = engine
                    .workspace_at(loc.ws_idx)
                    .expect("workspace index is valid");
                let terminals = &engine.runtime.terminals;
                crate::model::ClosedItem::from_workspace(
                    ws,
                    &mut snap_fn,
                    &|id| terminals.closed_capture(id),
                    presentation,
                )
            };
            close_trace::log_snapshot(t, &item, path);
            let t = Instant::now();
            engine.push_closed_item(item).log(t.elapsed(), path);
        }
        let t_collect = Instant::now();
        let mut targets: Vec<(u32, Option<String>)> = Vec::new();
        let mut closed_tab_ids: Vec<u32> = Vec::new();
        let mut closed_pane_ids: Vec<u32> = Vec::new();
        {
            let ws = engine
                .workspace_at(loc.ws_idx)
                .expect("workspace index is valid");
            for pid in ws.pane_layout().all_pane_ids() {
                closed_pane_ids.push(pid);
                if let Some(pane) = ws.pane_layout().find_pane(pid) {
                    for tab in &pane.tabs {
                        collect_close_targets(tab, &engine.as_ref(), &mut targets);
                        closed_tab_ids.push(tab.id);
                    }
                }
            }
        }
        close_trace::log_collect(t_collect, targets.len(), path);
        let workspace_id = engine
            .workspace_at(loc.ws_idx)
            .expect("workspace index is valid")
            .id;
        engine.remove_workspace_at(loc.ws_idx);
        let workspaces_now_empty = engine.workspaces().is_empty();
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

    pub(super) fn apply_close_tab(engine: &mut EngineMut<'_>, tab_id: u32) -> CoreEvent {
        Self::close_tab_recording(engine, tab_id, None)
    }

    /// 탭을 닫는다. save_snapshot이면 실제로 닫히는 탭만 복원 기록에 남긴다.
    /// pane의 마지막 탭은 닫지 않으므로 기록하지 않는다. 기록 여부는 origin을 아는 진입점이 정한다.
    pub(crate) fn close_tab_recording(
        engine: &mut EngineMut<'_>,
        tab_id: u32,
        presentation: Option<&dyn crate::model::StructurePresentation>,
    ) -> CoreEvent {
        let mut targets: Vec<(u32, Option<String>)> = Vec::new();
        let mut found_pane_id = None;
        for workspace in &engine.workspaces() {
            for &pid in &workspace.pane_layout().all_pane_ids() {
                if let Some(pane) = workspace.pane_layout().find_pane(pid)
                    && let Some(tab) = pane.tabs.iter().find(|t| t.id == tab_id)
                {
                    collect_close_targets(tab, &engine.as_ref(), &mut targets);
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

        if let Some(presentation) = presentation
            && let Some(pane) = engine.find_pane_by_id(pane_id)
            && pane.tabs.len() > 1
            && let Some(idx) = pane.tabs.iter().position(|t| t.id == tab_id)
            && let Some(item) = engine.capture_closed_tab(pane_id, idx, presentation)
        {
            engine.push_closed_item(item);
        }

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
    use crate::app::command::CascadeLevel;
    use tasty_terminal::Terminal;

    fn test_engine() -> crate::runtime::engine_session::EngineSession {
        let waker: tasty_terminal::Waker = std::sync::Arc::new(|| {});
        crate::runtime::engine_session::EngineSession::new(80, 24, waker).expect("engine")
    }

    fn insert_detached(engine: &mut EngineMut<'_>, sid: u32) {
        engine
            .runtime
            .terminals
            .insert(sid, Terminal::new_detached(80, 24), None);
    }

    #[test]
    fn case2_tab_close_returns_tab_level_fields() {
        let mut engine_session = test_engine();
        let mut engine = engine_session.borrow_mut();
        let sid0 = engine
            .workspace_at(0)
            .expect("workspace index is valid")
            .all_surface_ids()[0];
        let (ws_idx, pane_id) = engine.find_workspace_index_for_surface(sid0).unwrap();
        let tab1_id = engine.runtime.counters.next_tab();
        let sid1 = engine.runtime.counters.next_surface();
        insert_detached(&mut engine, sid1);
        engine
            .workspace_at_mut(ws_idx)
            .expect("workspace index is valid")
            .pane_layout_mut()
            .find_pane_mut(pane_id)
            .unwrap()
            .add_terminal_marker_tab(tab1_id, sid1);

        let ev = AppServices::apply_close_surface(&mut engine, sid1, None);
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
            engine
                .workspace_at(ws_idx)
                .expect("workspace index is valid")
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
        let mut engine_session = test_engine();
        let mut engine = engine_session.borrow_mut();
        let sid0 = engine
            .workspace_at(0)
            .expect("workspace index is valid")
            .all_surface_ids()[0];
        let (ws_idx, pane0) = engine.find_workspace_index_for_surface(sid0).unwrap();
        let pane1_id = engine.runtime.counters.next_pane();
        let tab1_id = engine.runtime.counters.next_tab();
        let sid1 = engine.runtime.counters.next_surface();
        insert_detached(&mut engine, sid1);
        let new_pane = crate::model::Pane::new_with_terminal_marker(pane1_id, tab1_id, sid1);
        let leftover = engine
            .workspace_at_mut(ws_idx)
            .expect("workspace index is valid")
            .pane_layout_mut()
            .split_pane_in_place(pane0, crate::model::SplitDirection::Horizontal, new_pane);
        assert!(leftover.is_none(), "split 성공해야 함");

        let ev = AppServices::apply_close_surface(&mut engine, sid1, None);
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
            engine
                .workspace_at(ws_idx)
                .expect("workspace index is valid")
                .pane_layout()
                .all_pane_ids()
                .len(),
            1
        );
    }

    #[test]
    fn case4_workspace_close_returns_workspace_level_fields() {
        let mut engine_session = test_engine();
        let mut engine = engine_session.borrow_mut();
        let ws1_id = engine.runtime.counters.next_workspace();
        let pane1_id = engine.runtime.counters.next_pane();
        let tab1_id = engine.runtime.counters.next_tab();
        let sid1 = engine.runtime.counters.next_surface();
        insert_detached(&mut engine, sid1);
        let ws1 = crate::model::Workspace::new_with_terminal_marker(
            ws1_id,
            "ws1".to_string(),
            pane1_id,
            tab1_id,
            sid1,
        );
        engine.push_local_workspace(ws1);
        assert_eq!(engine.workspaces().len(), 2);

        let ev = AppServices::apply_close_surface(&mut engine, sid1, None);
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
        assert_eq!(engine.workspaces().len(), 1);
    }

    /// 앞쪽 탭 삭제 후에도 active_tab 인덱스가 원래 보던 탭을 가리켜야 한다.
    #[test]
    fn case2_tab_close_preserves_the_viewed_tab() {
        let mut navigation = crate::state::navigation::NavigationState::default();
        let mut engine_session = test_engine();
        let mut engine = engine_session.borrow_mut();
        let sid0 = engine
            .workspace_at(0)
            .expect("workspace index is valid")
            .all_surface_ids()[0];
        let (ws_idx, pane_id) = engine.find_workspace_index_for_surface(sid0).unwrap();
        let mut tab_ids = vec![];
        for _ in 0..2 {
            let tab_id = engine.runtime.counters.next_tab();
            let sid = engine.runtime.counters.next_surface();
            insert_detached(&mut engine, sid);
            engine
                .workspace_at_mut(ws_idx)
                .expect("workspace index is valid")
                .pane_layout_mut()
                .find_pane_mut(pane_id)
                .unwrap()
                .add_terminal_marker_tab(tab_id, sid);
            tab_ids.push(tab_id);
        }
        let pane = engine
            .workspace_at_mut(ws_idx)
            .expect("workspace index is valid")
            .pane_layout_mut()
            .find_pane_mut(pane_id)
            .unwrap();
        navigation.goto_tab(pane, 1);
        let viewed_tab_id = pane.tabs[1].id;

        let ev = AppServices::apply_close_surface(&mut engine, sid0, None);
        navigation.apply_result(&engine.workspaces(), &ev);
        assert!(matches!(ev, CoreEvent::SurfaceClosed { closed: true, .. }));

        let pane = engine
            .workspace_at(ws_idx)
            .expect("workspace index is valid")
            .pane_layout()
            .find_pane(pane_id)
            .unwrap();
        assert_eq!(pane.tabs.len(), 2);
        assert_eq!(
            pane.tabs[navigation.tab_index(pane)].id,
            viewed_tab_id,
            "앞쪽 탭이 닫혀도 보던 탭이 유지돼야 한다"
        );
    }

    #[test]
    fn case3_pane_close_keeps_focus_on_an_untouched_pane() {
        let mut engine_session = test_engine();
        let mut engine = engine_session.borrow_mut();
        let sid0 = engine
            .workspace_at_mut(0)
            .expect("workspace index is valid")
            .all_surface_ids()[0];
        let (ws_idx, pane0) = engine.find_workspace_index_for_surface(sid0).unwrap();
        let pane1_id = engine.runtime.counters.next_pane();
        let tab1_id = engine.runtime.counters.next_tab();
        let sid1 = engine.runtime.counters.next_surface();
        insert_detached(&mut engine, sid1);
        let new_pane = crate::model::Pane::new_with_terminal_marker(pane1_id, tab1_id, sid1);
        let leftover = engine
            .workspace_at_mut(ws_idx)
            .expect("workspace index is valid")
            .pane_layout_mut()
            .split_pane_in_place(pane0, crate::model::SplitDirection::Horizontal, new_pane);
        assert!(leftover.is_none());
        // 첫 pane으로 무조건 옮기는 오류를 잡으려면 포커스를 다른 pane에 두어야 한다.
        let pane2_id = engine.runtime.counters.next_pane();
        let tab2_id = engine.runtime.counters.next_tab();
        let sid2 = engine.runtime.counters.next_surface();
        insert_detached(&mut engine, sid2);
        let third = crate::model::Pane::new_with_terminal_marker(pane2_id, tab2_id, sid2);
        let leftover = engine
            .workspace_at_mut(ws_idx)
            .expect("workspace index is valid")
            .pane_layout_mut()
            .split_pane_in_place(pane1_id, crate::model::SplitDirection::Horizontal, third);
        assert!(leftover.is_none());
        assert_eq!(
            engine
                .workspace_at(ws_idx)
                .expect("workspace index is valid")
                .pane_layout()
                .all_pane_ids()
                .len(),
            3
        );
        let mut navigation = crate::state::navigation::NavigationState::default();
        navigation.select_pane(
            engine
                .workspace_at(ws_idx)
                .expect("workspace index is valid"),
            pane2_id,
        );

        let ev = AppServices::apply_close_surface(&mut engine, sid0, None);
        navigation.apply_result(&engine.workspaces(), &ev);
        assert!(matches!(ev, CoreEvent::SurfaceClosed { closed: true, .. }));

        assert_eq!(
            navigation
                .pane_id(
                    engine
                        .workspace_at(ws_idx)
                        .expect("workspace index is valid")
                )
                .unwrap(),
            pane2_id,
            "포커스와 무관한 pane 이 닫혔는데 포커스가 움직이면 안 된다"
        );
    }

    #[test]
    fn case4_workspace_close_reports_the_removed_index() {
        let mut engine_session = test_engine();
        let mut engine = engine_session.borrow_mut();
        let ws1_id = engine.runtime.counters.next_workspace();
        let pane1_id = engine.runtime.counters.next_pane();
        let tab1_id = engine.runtime.counters.next_tab();
        let sid1 = engine.runtime.counters.next_surface();
        insert_detached(&mut engine, sid1);
        engine.insert_local_workspace(
            0,
            crate::model::Workspace::new_with_terminal_marker(
                ws1_id,
                "ws1".to_string(),
                pane1_id,
                tab1_id,
                sid1,
            ),
        );

        let ev = AppServices::apply_close_surface(&mut engine, sid1, None);
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
        let mut engine_session = test_engine();
        let mut engine = engine_session.borrow_mut();
        let sid0 = engine
            .workspace_at(0)
            .expect("workspace index is valid")
            .all_surface_ids()[0];
        insert_detached(&mut engine, sid0);
        let ws0_id = engine.workspace_at(0).expect("workspace index is valid").id;

        let ev = AppServices::apply_close_surface(&mut engine, sid0, None);
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
        assert!(engine.workspaces().is_empty());
    }

    #[test]
    fn case1_split_close_returns_single_cleanup_target() {
        let mut engine_session = test_engine();
        let mut engine = engine_session.borrow_mut();
        let sid_a = engine
            .workspace_at(0)
            .expect("workspace index is valid")
            .all_surface_ids()[0];
        insert_detached(&mut engine, sid_a);
        let (ws_idx, pane_id) = engine.find_workspace_index_for_surface(sid_a).unwrap();
        let sid_b = engine.runtime.counters.next_surface();
        engine
            .workspace_at_mut(ws_idx)
            .expect("workspace index is valid")
            .pane_layout_mut()
            .find_pane_mut(pane_id)
            .unwrap()
            .split_surface_by_id_marker(sid_a, crate::model::SplitDirection::Horizontal, sid_b)
            .unwrap();
        insert_detached(&mut engine, sid_b);

        let ev = AppServices::apply_close_surface(&mut engine, sid_a, None);
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
