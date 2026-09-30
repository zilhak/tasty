//! 우클릭 메뉴 대상. 메뉴 결과는 메뉴를 연 뒤에 도착하므로 그 사이 바뀐 순서를 반영해 다시 찾는다.

use crate::core::CoreState;
use crate::core::engine_access::EngineMut;

/// workspace 메뉴를 연 대상. 인덱스는 에이전트의 닫기·이동으로 다른 workspace를 가리킬 수 있어 ID로 보관한다.
#[derive(Debug, Clone, Copy)]
pub(super) struct WorkspaceMenuTarget {
    workspace_id: Option<u32>,
}

impl WorkspaceMenuTarget {
    pub(super) fn capture(engine: &CoreState, ws_idx: usize) -> Self {
        Self {
            workspace_id: engine.workspaces.get(ws_idx).map(|w| w.id),
        }
    }

    /// 대상의 현재 인덱스. 대상이 사라졌으면 None이다.
    pub(super) fn resolve(&self, engine: &CoreState) -> Option<usize> {
        engine.find_workspace_index_for_id(self.workspace_id?)
    }
}

/// 탭 메뉴를 연 대상. 탭 순서가 바뀌어도 같은 탭을 가리키도록 ID로 보관한다.
#[derive(Debug, Clone, Copy)]
pub(super) struct TabMenuTarget {
    tab_id: Option<u32>,
}

impl TabMenuTarget {
    pub(super) fn capture(engine: &CoreState, pane_id: u32, tab_index: usize) -> Self {
        Self {
            tab_id: engine
                .find_pane_by_id(pane_id)
                .and_then(|p| p.tabs.get(tab_index))
                .map(|t| t.id),
        }
    }

    pub(super) fn tab_id(&self) -> Option<u32> {
        self.tab_id
    }

    /// 대상의 현재 (pane_id, 탭 인덱스). 대상이 사라졌으면 None이다.
    pub(super) fn resolve(&self, engine: &CoreState) -> Option<(u32, usize)> {
        let tab_id = self.tab_id?;
        let pane_id = engine.find_pane_for_tab(tab_id)?;
        let index = engine
            .find_pane_by_id(pane_id)?
            .tabs
            .iter()
            .position(|t| t.id == tab_id)?;
        Some((pane_id, index))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::tests::test_state;

    fn push_workspace(engine: &mut EngineMut<'_>) -> u32 {
        let event = crate::core::apply_create_workspace_inner(
            engine,
            crate::core::WorkspaceCreationParams::terminal(),
        )
        .expect("create workspace");
        let crate::core::intent::CoreEvent::WorkspaceCreated { id, .. } = event else {
            panic!("apply_create_workspace_inner must return WorkspaceCreated");
        };
        id
    }

    /// 메뉴가 열린 동안 에이전트가 앞쪽 workspace를 닫아도 메뉴는 연 workspace를 가리킨다.
    #[test]
    fn workspace_target_follows_its_workspace_after_agent_close() {
        let (mut state, mut engine) = test_state();
        let a = push_workspace(&mut engine);
        let b = push_workspace(&mut engine);
        let target =
            WorkspaceMenuTarget::capture(&engine, engine.find_workspace_index_for_id(a).unwrap());

        assert!(state.close_workspace_at(
            &mut engine,
            0,
            crate::state::WorkspaceCloseOrigin::Agent
        ));
        assert_eq!(
            target.resolve(&engine),
            engine.find_workspace_index_for_id(a)
        );
        assert_ne!(
            target.resolve(&engine),
            engine.find_workspace_index_for_id(b)
        );
    }

    /// 메뉴가 열린 동안 workspace 순서가 바뀌어도 메뉴는 연 workspace를 가리킨다.
    #[test]
    fn workspace_target_follows_its_workspace_after_reorder() {
        let (mut state, mut engine) = test_state();
        let a = push_workspace(&mut engine);
        let target =
            WorkspaceMenuTarget::capture(&engine, engine.find_workspace_index_for_id(a).unwrap());

        let from = engine.find_workspace_index_for_id(a).unwrap();
        state.move_workspace(&mut engine, from, 0);
        assert_eq!(target.resolve(&engine), Some(0));
    }

    /// 대상 workspace가 닫혔으면 다른 workspace로 넘어가지 않는다.
    #[test]
    fn workspace_target_is_gone_after_its_workspace_closes() {
        let (mut state, mut engine) = test_state();
        push_workspace(&mut engine);
        push_workspace(&mut engine);
        let target = WorkspaceMenuTarget::capture(&engine, 1);

        assert!(state.close_workspace_at(
            &mut engine,
            1,
            crate::state::WorkspaceCloseOrigin::Agent
        ));
        assert_eq!(target.resolve(&engine), None);
    }

    /// 메뉴가 열린 동안 탭 순서가 바뀌어도 메뉴는 연 탭을 가리킨다.
    #[test]
    fn tab_target_follows_its_tab_after_reorder() {
        let (mut state, mut engine) = test_state();
        state.add_tab(&mut engine).unwrap();
        state.add_tab(&mut engine).unwrap();
        let pane_id = state.focused_pane_id(&engine);
        let first = engine.find_pane_by_id(pane_id).unwrap().tabs[0].id;
        let middle = engine.find_pane_by_id(pane_id).unwrap().tabs[1].id;
        let target = TabMenuTarget::capture(&engine, pane_id, 1);

        let mut core = crate::adapters::ipc::handler::cli_entry_tests::test_core();
        core.apply(
            &mut engine,
            crate::core::intent::DomainIntent::MoveTab {
                pane_id,
                tab_id: first,
                to_index: 2,
            },
        )
        .expect("move tab");
        let (pid, index) = target.resolve(&engine).expect("tab still exists");
        assert_eq!(pid, pane_id);
        assert_eq!(engine.find_pane_by_id(pid).unwrap().tabs[index].id, middle);
    }

    /// 대상 탭이 닫혔으면 다른 탭으로 넘어가지 않는다.
    #[test]
    fn tab_target_is_gone_after_its_tab_closes() {
        let (mut state, mut engine) = test_state();
        state.add_tab(&mut engine).unwrap();
        let pane_id = state.focused_pane_id(&engine);
        let first = engine.find_pane_by_id(pane_id).unwrap().tabs[0].id;
        let target = TabMenuTarget::capture(&engine, pane_id, 0);

        let mut core = crate::adapters::ipc::handler::cli_entry_tests::test_core();
        core.apply(
            &mut engine,
            crate::core::intent::DomainIntent::CloseTab { tab_id: first },
        )
        .expect("close tab");
        assert_eq!(target.resolve(&engine), None);
    }
}
