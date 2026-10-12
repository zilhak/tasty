//! App applies builtin kind changes after the readonly drawing phase.
use super::requests::explorer::{BoundExplorerCommand, ExplorerCommand, ExplorerCommandOutcome};
use crate::runtime::engine_access::EngineMut;

/// 대상 binding이 아직 같은 탐색기를 가리킬 때만 적용하고 레이아웃을 dirty로 표시한다.
pub(super) fn apply_bound_explorer_command(
    engine: &mut EngineMut<'_>,
    request: &BoundExplorerCommand,
) -> ExplorerCommandOutcome {
    let target = request.target();
    if !target.current(&engine.as_ref()) {
        return ExplorerCommandOutcome::StaleTarget;
    }
    let Some(panel) = engine
        .runtime
        .surfaces
        .get_mut(&target.surface_id())
        .and_then(|surface| {
            surface
                .as_any_mut()
                .downcast_mut::<crate::model::ExplorerPanel>()
        })
    else {
        return ExplorerCommandOutcome::NotExplorer;
    };
    apply_to_explorer_panel(panel, request.command());
    engine.mark_layout_dirty();
    ExplorerCommandOutcome::Applied
}

fn apply_to_explorer_panel(ex: &mut crate::model::ExplorerPanel, command: &ExplorerCommand) {
    use ExplorerCommand as C;
    match command {
        C::Navigate(p) => {
            ex.active_tab_mut().navigate_to(p.clone());
        }
        C::Back => {
            ex.active_tab_mut().go_back();
        }
        C::Forward => {
            ex.active_tab_mut().go_forward();
        }
        C::Up => {
            ex.active_tab_mut().go_up();
        }
        C::ViewMode(m) => {
            ex.active_tab_mut().view_mode = *m;
        }
        C::Sort(col) => {
            let tab = ex.active_tab_mut();
            if tab.sort_column == *col {
                tab.sort_dir = tab.sort_dir.toggled();
            } else {
                tab.sort_column = *col;
                tab.sort_dir = crate::model::SortDir::Asc;
            }
        }
        C::NewTab => ex.add_tab(),
        C::CloseTab(i) => ex.close_tab(*i),
        C::SelectTab(i) => {
            if *i < ex.tabs.len() {
                ex.active = *i;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::apply_bound_explorer_command;
    use crate::app::requests::explorer::{
        BoundExplorerCommand, ExplorerCommand as C, ExplorerCommandOutcome as Outcome,
    };
    use crate::model::{ExplorerPanel, ExplorerViewMode, SortColumn, SortDir};
    use crate::runtime::engine_session::EngineSession;
    use crate::runtime::surface_binding::SurfaceBinding;

    fn panel(session: &mut EngineSession, sid: u32) -> &ExplorerPanel {
        session
            .runtime
            .surfaces
            .get_mut(&sid)
            .and_then(|surface| surface.as_any_mut().downcast_mut::<ExplorerPanel>())
            .expect("explorer panel")
    }

    fn send(session: &mut EngineSession, target: &SurfaceBinding, command: C) -> Outcome {
        apply_bound_explorer_command(
            &mut session.borrow_mut(),
            &BoundExplorerCommand::new(target.clone(), command),
        )
    }

    /// 모든 명령이 활성 내부 탭 또는 내부 탭 목록을 바꾸고 레이아웃을 dirty로 표시한다. 탭은 index로 고른다.
    #[test]
    fn every_command_changes_the_bound_explorer_and_marks_the_layout_dirty() {
        let (_state, mut session, sid, root) = crate::state::tests::explorer_fixture(false);
        let target = SurfaceBinding::capture(&session.read(), sid).expect("binding");
        assert!(session.persistence.dirty.dirty_since().is_none());

        let sub = root.join("sub");
        assert_eq!(
            send(&mut session, &target, C::Navigate(sub.clone())),
            Outcome::Applied
        );
        assert!(session.persistence.dirty.dirty_since().is_some());
        assert_eq!(panel(&mut session, sid).current_root(), sub.as_path());
        send(&mut session, &target, C::Back);
        assert_eq!(panel(&mut session, sid).current_root(), root.as_path());
        send(&mut session, &target, C::Forward);
        assert_eq!(panel(&mut session, sid).current_root(), sub.as_path());
        send(&mut session, &target, C::Up);
        assert_eq!(panel(&mut session, sid).current_root(), root.as_path());

        send(&mut session, &target, C::ViewMode(ExplorerViewMode::Detail));
        assert_eq!(
            panel(&mut session, sid).active_tab().view_mode,
            ExplorerViewMode::Detail
        );
        let first = panel(&mut session, sid).active_tab().sort_column;
        let other = if first == SortColumn::Size {
            SortColumn::Name
        } else {
            SortColumn::Size
        };
        send(&mut session, &target, C::Sort(other));
        let tab = panel(&mut session, sid).active_tab();
        assert_eq!((tab.sort_column, tab.sort_dir), (other, SortDir::Asc));
        send(&mut session, &target, C::Sort(other));
        let tab = panel(&mut session, sid).active_tab();
        assert_eq!((tab.sort_column, tab.sort_dir), (other, SortDir::Desc));

        send(&mut session, &target, C::NewTab);
        assert_eq!(
            (
                panel(&mut session, sid).tabs.len(),
                panel(&mut session, sid).active
            ),
            (2, 1)
        );
        send(&mut session, &target, C::SelectTab(0));
        assert_eq!(panel(&mut session, sid).active, 0);
        send(&mut session, &target, C::SelectTab(5));
        assert_eq!(
            panel(&mut session, sid).active,
            0,
            "out of range keeps the tab"
        );
        send(&mut session, &target, C::CloseTab(1));
        assert_eq!(
            (
                panel(&mut session, sid).tabs.len(),
                panel(&mut session, sid).active
            ),
            (1, 0)
        );
    }

    /// 요청 뒤에 대상이 닫히거나 projection이 다시 만들어지면 패널과 dirty 표시를 바꾸지 않는다.
    #[test]
    fn a_closed_or_replaced_target_is_rejected_without_touching_the_panel() {
        let (_state, mut session, sid, root) = crate::state::tests::explorer_fixture(true);
        let target = SurfaceBinding::capture(&session.read(), sid).expect("binding");
        let workspace = session
            .core_state
            .remove_mirror_workspace(sid)
            .expect("mirror workspace");

        let away = root.join("away");
        assert_eq!(
            send(&mut session, &target, C::Navigate(away.clone())),
            Outcome::StaleTarget,
            "closed surface"
        );
        session.core_state.push_mirror_workspace(workspace);
        assert_eq!(
            send(&mut session, &target, C::Navigate(away.clone())),
            Outcome::StaleTarget,
            "replaced projection"
        );
        assert_eq!(panel(&mut session, sid).current_root(), root.as_path());
        assert!(session.persistence.dirty.dirty_since().is_none());

        let fresh = SurfaceBinding::capture(&session.read(), sid).expect("binding");
        assert_eq!(
            send(&mut session, &fresh, C::Navigate(away.clone())),
            Outcome::Applied
        );
        assert_eq!(panel(&mut session, sid).current_root(), away.as_path());
    }

    /// 같은 세대의 surface여도 탐색기가 아니면 적용하지 않는다.
    #[test]
    fn a_surface_that_is_not_an_explorer_is_rejected() {
        let (_state, mut session) = crate::state::tests::test_state();
        let sid = session.read().workspace_at(0).unwrap().all_surface_ids()[0];
        let target = SurfaceBinding::capture(&session.read(), sid).expect("binding");
        assert_eq!(send(&mut session, &target, C::NewTab), Outcome::NotExplorer);
        assert!(session.persistence.dirty.dirty_since().is_none());
    }
}
