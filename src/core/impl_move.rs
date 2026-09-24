//! surface를 다른 surface 위치로 옮긴다. 원본 터미널은 유지하고 덮어쓴 대상의 후속 정리를 반환한다.

use super::*;

impl Core {
    /// source를 트리에서 떼어 target 위치에 붙인다. Terminal store는 여기서 지우지 않는다.
    /// target 정리는 반환된 이벤트의 호출자가 맡는다. 성공하지 않아도 cut 슬롯은 비운다.
    pub(super) fn apply_move_surface(
        engine: &mut crate::core::CoreState,
        source_id: u32,
        target_id: u32,
    ) -> CoreEvent {
        use crate::core::intent::CascadeLevel;

        engine.pending_move_surface = None;

        let noop = || CoreEvent::MoveSurfaceApplied {
            moved: false,
            b_cleanup: None,
            cascade_level: CascadeLevel::Surface,
            closed_tab_ids: vec![],
            closed_pane_ids: vec![],
            workspace_purged: None,
            workspaces_now_empty: false,
        };

        if source_id == target_id {
            return noop();
        }
        if engine.find_workspace_index_for_surface(source_id).is_none() {
            return noop();
        }
        if engine.find_workspace_index_for_surface(target_id).is_none() {
            return noop();
        }

        let (
            a_box,
            cascade_level,
            closed_tab_ids,
            closed_pane_ids,
            workspace_purged,
            workspaces_now_empty,
        ) = match Self::detach_surface_for_move(engine, source_id) {
            Some(v) => v,
            None => return noop(),
        };

        Self::attach_a_to_target(
            engine,
            source_id,
            target_id,
            a_box,
            cascade_level,
            closed_tab_ids,
            closed_pane_ids,
            workspace_purged,
            workspaces_now_empty,
        )
    }

    /// source를 떼는 동안 위치가 바뀔 수 있어 target을 ID로 다시 찾는다.
    #[allow(clippy::too_many_arguments)]
    fn attach_a_to_target(
        engine: &mut crate::core::CoreState,
        source_id: u32,
        target_id: u32,
        a_box: Box<dyn crate::model::Surface>,
        cascade_level: crate::core::intent::CascadeLevel,
        closed_tab_ids: Vec<u32>,
        closed_pane_ids: Vec<u32>,
        workspace_purged: Option<(usize, u32)>,
        workspaces_now_empty: bool,
    ) -> CoreEvent {
        // 실패 결과에도 이미 지운 tab·pane·workspace 정보를 싣는다.
        // 호출자의 moved 검사 때문에 이 정보로 후속 정리가 실행되지 않을 수 있다.
        let fail =
            |closed_tab_ids: &[u32], closed_pane_ids: &[u32]| CoreEvent::MoveSurfaceApplied {
                moved: false,
                b_cleanup: None,
                cascade_level,
                closed_tab_ids: closed_tab_ids.to_vec(),
                closed_pane_ids: closed_pane_ids.to_vec(),
                workspace_purged,
                workspaces_now_empty,
            };

        let Some((ws_idx, pane_id, b_tab_idx, b_persist)) =
            Self::locate_target_slot(engine, source_id, target_id)
        else {
            return fail(&closed_tab_ids, &closed_pane_ids);
        };

        // target의 트리 항목만 교체한다. Terminal store 정리는 반환 이벤트로 요청한다.
        let replaced = Self::replace_b_with_a(engine, ws_idx, pane_id, b_tab_idx, target_id, a_box);
        if !replaced {
            tracing::error!(
                source_id,
                target_id,
                "move surface: failed to replace target B"
            );
            return fail(&closed_tab_ids, &closed_pane_ids);
        }

        // split leaf 교체는 focused_surface를 바꾸지 않으므로 직접 이어주고 제목도 갱신한다.
        Self::transfer_focus_to_a(engine, ws_idx, pane_id, b_tab_idx, source_id, target_id);
        engine.mark_layout_dirty();
        engine.refresh_tab_osc_title(source_id);

        CoreEvent::MoveSurfaceApplied {
            moved: true,
            b_cleanup: Some((target_id, b_persist)),
            cascade_level,
            closed_tab_ids,
            closed_pane_ids,
            workspace_purged,
            workspaces_now_empty,
        }
    }

    /// source 제거가 인덱스를 바꿀 수 있어 target 위치와 scrollback 저장 ID를 다시 조회한다.
    fn locate_target_slot(
        engine: &crate::core::CoreState,
        source_id: u32,
        target_id: u32,
    ) -> Option<(usize, u32, usize, Option<String>)> {
        let (ws_idx, pane_id) = match engine.find_workspace_index_for_surface(target_id) {
            Some(v) => v,
            None => {
                tracing::error!(
                    source_id,
                    target_id,
                    "move surface: target not found after detaching source"
                );
                return None;
            }
        };
        let b_persist = engine
            .terminals
            .scrollback_persist_id(target_id)
            .map(str::to_string);
        let b_tab_idx = {
            let ws = &engine.workspaces[ws_idx];
            match ws.pane_layout().find_pane(pane_id) {
                Some(pane) => pane.tabs.iter().position(|t| t.contains_surface(target_id)),
                None => None,
            }
        };
        let b_tab_idx = match b_tab_idx {
            Some(i) => i,
            None => {
                tracing::error!(source_id, target_id, "move surface: target B tab not found");
                return None;
            }
        };
        Some((ws_idx, pane_id, b_tab_idx, b_persist))
    }

    fn replace_b_with_a(
        engine: &mut crate::core::CoreState,
        ws_idx: usize,
        pane_id: u32,
        b_tab_idx: usize,
        target_id: u32,
        a_box: Box<dyn crate::model::Surface>,
    ) -> bool {
        let ws = &mut engine.workspaces[ws_idx];
        let pane = ws
            .pane_layout_mut()
            .find_pane_mut(pane_id)
            .expect("pane re-search must hit (just found above)");
        let tab = &mut pane.tabs[b_tab_idx];
        if tab.is_split() {
            tab.layout_mut().replace_surface(target_id, a_box)
        } else {
            tab.put_surface(a_box);
            true
        }
    }

    fn transfer_focus_to_a(
        engine: &mut crate::core::CoreState,
        ws_idx: usize,
        pane_id: u32,
        b_tab_idx: usize,
        source_id: u32,
        target_id: u32,
    ) {
        let ws = &mut engine.workspaces[ws_idx];
        if let Some(pane) = ws.pane_layout_mut().find_pane_mut(pane_id) {
            let tab = &mut pane.tabs[b_tab_idx];
            if tab.focused_surface == target_id {
                tab.focused_surface = source_id;
            }
        }
    }

    /// source의 Box를 떼어 반환한다. 비게 된 tab·pane·workspace는 지우되
    /// 이동할 Terminal store 항목과 scrollback은 유지하며 닫기 snapshot도 만들지 않는다.
    #[allow(clippy::type_complexity)]
    fn detach_surface_for_move(
        engine: &mut crate::core::CoreState,
        source_id: u32,
    ) -> Option<(
        Box<dyn crate::model::Surface>,
        crate::core::intent::CascadeLevel,
        Vec<u32>,
        Vec<u32>,
        // 제거한 workspace의 (인덱스, ID). 호출자가 활성 workspace 위치를 보정할 때 쓴다.
        Option<(usize, u32)>,
        bool,
    )> {
        use crate::core::intent::CascadeLevel;

        let (ws_idx, pane_id) = engine.find_workspace_index_for_surface(source_id)?;

        let (tab_idx, is_split) = {
            let ws = &engine.workspaces[ws_idx];
            let pane = ws.pane_layout().find_pane(pane_id)?;
            let mut found = None;
            for (i, tab) in pane.tabs.iter().enumerate() {
                if tab.contains_surface(source_id) {
                    found = Some((i, tab.is_split()));
                    break;
                }
            }
            found?
        };

        if is_split {
            let (a_box, source_tab_focused) = {
                let ws = &mut engine.workspaces[ws_idx];
                let pane = ws.pane_layout_mut().find_pane_mut(pane_id)?;
                let tab = &mut pane.tabs[tab_idx];
                let layout = tab.take_layout();
                let (new_layout, extracted) = layout.extract_surface(source_id);
                tab.put_layout(new_layout);
                // 떠난 source를 계속 선택하지 않도록 남은 surface로 바꾼다.
                if tab.focused_surface == source_id
                    && let Some(first_id) = tab.layout().first_surface_id()
                {
                    tab.focused_surface = first_id;
                }
                let a_box = extracted?;
                (a_box, tab.focused_surface)
            };
            engine.mark_layout_dirty();
            // source의 옛 제목이 남은 탭에 남지 않도록 다시 계산한다.
            engine.refresh_tab_osc_title(source_tab_focused);
            return Some((a_box, CascadeLevel::Surface, vec![], vec![], None, false));
        }

        let (tabs_len, panes_len, tab_id) = {
            let ws = &engine.workspaces[ws_idx];
            let pane = ws.pane_layout().find_pane(pane_id)?;
            (
                pane.tabs.len(),
                ws.pane_layout().all_pane_ids().len(),
                pane.tabs[tab_idx].id,
            )
        };

        // take_layout 뒤에는 잠시 layout이 없다. 같은 동기 호출 안에서 tab을 제거하거나 돌려놓는다.
        let a_box = {
            let ws = &mut engine.workspaces[ws_idx];
            let pane = ws.pane_layout_mut().find_pane_mut(pane_id)?;
            let tab = &mut pane.tabs[tab_idx];
            match tab.take_layout() {
                crate::model::SurfaceLayout::Leaf(b) => b,
                other => {
                    tab.put_layout(other);
                    return None;
                }
            }
        };

        if tabs_len > 1 {
            let ws = &mut engine.workspaces[ws_idx];
            let pane = ws.pane_layout_mut().find_pane_mut(pane_id)?;
            pane.remove_tab_preserving_active(tab_idx);
            engine.mark_layout_dirty();
            return Some((a_box, CascadeLevel::Tab, vec![tab_id], vec![], None, false));
        }

        if panes_len > 1 {
            let ws = &mut engine.workspaces[ws_idx];
            ws.close_pane_preserving_focus(pane_id);
            engine.mark_layout_dirty();
            return Some((
                a_box,
                CascadeLevel::Pane,
                vec![tab_id],
                vec![pane_id],
                None,
                false,
            ));
        }

        let workspace_id = engine.workspaces[ws_idx].id;
        engine.workspaces.remove(ws_idx);
        let workspaces_now_empty = engine.workspaces.is_empty();
        engine.mark_layout_dirty();
        Some((
            a_box,
            CascadeLevel::Workspace,
            vec![tab_id],
            vec![pane_id],
            Some((ws_idx, workspace_id)),
            workspaces_now_empty,
        ))
    }
}

#[cfg(test)]
mod move_surface_tests {
    use super::*;
    use crate::model::SplitDirection;

    fn test_engine() -> CoreState {
        let waker: tasty_terminal::Waker = std::sync::Arc::new(|| {});
        CoreState::new(80, 24, waker).expect("engine")
    }

    /// 실제 PTY 대신 detached Terminal을 써서 store 항목과 정리 대상 반환을 확인한다.
    #[test]
    fn move_preserves_source_terminal_and_reports_b_cleanup() {
        let mut engine = test_engine();
        let a = engine.workspaces[0].all_surface_ids()[0];
        engine
            .terminals
            .insert(a, tasty_terminal::Terminal::new_detached(80, 24));

        let b = 7777;
        let (ws_idx, pane_id) = engine.find_workspace_index_for_surface(a).unwrap();
        engine.workspaces[ws_idx]
            .pane_layout_mut()
            .find_pane_mut(pane_id)
            .unwrap()
            .split_surface_by_id_marker(a, SplitDirection::Horizontal, b)
            .unwrap();
        engine
            .terminals
            .insert(b, tasty_terminal::Terminal::new_detached(80, 24));

        assert!(engine.terminals.contains(a));
        assert!(engine.terminals.contains(b));
        assert!(engine.find_workspace_index_for_surface(b).is_some());

        let ev = Core::apply_move_surface(&mut engine, a, b);

        match ev {
            CoreEvent::MoveSurfaceApplied {
                moved, b_cleanup, ..
            } => {
                assert!(moved, "move must succeed");
                assert_eq!(b_cleanup.map(|(id, _)| id), Some(b));
            }
            other => panic!("unexpected event: {other:?}"),
        }

        assert!(
            engine.terminals.contains(a),
            "source terminal must survive move"
        );
        assert!(engine.find_workspace_index_for_surface(a).is_some());
        // target의 store 제거는 호출자의 후속 처리이므로 여기서는 아직 남아 있어야 한다.
        assert!(engine.find_workspace_index_for_surface(b).is_none());
        assert!(
            engine.terminals.contains(b),
            "apply 직후에는 B의 Terminal store 항목도 남아 있어야 한다"
        );
        assert!(engine.pending_move_surface.is_none());
    }

    #[test]
    fn move_self_ref_is_noop() {
        let mut engine = test_engine();
        let a = engine.workspaces[0].all_surface_ids()[0];
        engine.pending_move_surface = Some(a);
        let ev = Core::apply_move_surface(&mut engine, a, a);
        assert!(matches!(
            ev,
            CoreEvent::MoveSurfaceApplied { moved: false, .. }
        ));
        assert!(engine.pending_move_surface.is_none());
    }

    #[test]
    fn move_missing_target_is_noop() {
        let mut engine = test_engine();
        let a = engine.workspaces[0].all_surface_ids()[0];
        engine
            .terminals
            .insert(a, tasty_terminal::Terminal::new_detached(80, 24));
        engine.pending_move_surface = Some(a);

        let ev = Core::apply_move_surface(&mut engine, a, 999_999);
        assert!(matches!(
            ev,
            CoreEvent::MoveSurfaceApplied { moved: false, .. }
        ));
        assert!(engine.terminals.contains(a));
        assert!(engine.find_workspace_index_for_surface(a).is_some());
        assert!(engine.pending_move_surface.is_none());
    }
}
