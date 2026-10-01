//! 우클릭 메뉴 대상. 메뉴 결과는 메뉴를 연 뒤에 도착하므로 그 사이 바뀐 순서를 반영해 다시 찾는다.

use crate::core::CoreState;

/// workspace 메뉴를 연 대상. 인덱스는 에이전트의 닫기·이동으로 다른 workspace를 가리킬 수 있어 ID로 보관한다.
#[derive(Debug, Clone, Copy)]
pub(super) struct WorkspaceMenuTarget {
    workspace_id: Option<u32>,
}

impl WorkspaceMenuTarget {
    pub(super) fn capture(engine: &CoreState, ws_idx: usize) -> Self {
        Self {
            workspace_id: engine.workspace_at(ws_idx).map(|w| w.id),
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
    use tasty_core::{DomainEvent as E, SurfaceSpec};

    fn fixture(extra: Vec<E>) -> crate::runtime::engine_session::EngineSession {
        let mut events = vec![E::CategoryCreated {
            id: 0,
            name: "normal".into(),
            index: 0,
        }];
        for id in 1..=3 {
            events.push(E::WorkspaceCreated {
                id,
                name: format!("workspace {id}"),
                category: 0,
                index: (id - 1) as usize,
                pane: id,
            });
            events.push(E::TabCreated {
                id,
                pane: id,
                index: 0,
                name: "first".into(),
                surface: SurfaceSpec {
                    id,
                    kind: "terminal".into(),
                    data: None,
                },
            });
        }
        for id in 4..=5 {
            events.push(E::TabCreated {
                id,
                pane: 1,
                index: (id - 3) as usize,
                name: "extra".into(),
                surface: SurfaceSpec {
                    id,
                    kind: "terminal".into(),
                    data: None,
                },
            });
        }
        events.extend(extra);
        crate::state::tests::test_state_from_model(crate::state::tests::test_model(events)).1
    }

    #[test]
    fn workspace_target_follows_its_workspace_after_agent_close() {
        let before = fixture(vec![]);
        let target = WorkspaceMenuTarget::capture(&before.core_state, 1);
        let after = fixture(vec![E::WorkspaceClosed { id: 1 }]);
        assert_eq!(target.resolve(&after.core_state), Some(0));
        assert_eq!(after.core_state.workspace_at(0).unwrap().id, 2);
    }

    #[test]
    fn workspace_target_follows_its_workspace_after_reorder() {
        let before = fixture(vec![]);
        let target = WorkspaceMenuTarget::capture(&before.core_state, 1);
        let after = fixture(vec![E::WorkspaceMoved {
            id: 2,
            category: 0,
            index: 0,
        }]);
        assert_eq!(target.resolve(&after.core_state), Some(0));
    }

    #[test]
    fn workspace_target_is_gone_after_its_workspace_closes() {
        let before = fixture(vec![]);
        let target = WorkspaceMenuTarget::capture(&before.core_state, 1);
        let after = fixture(vec![E::WorkspaceClosed { id: 2 }]);
        assert_eq!(target.resolve(&after.core_state), None);
    }

    #[test]
    fn tab_target_follows_its_tab_after_reorder() {
        let before = fixture(vec![]);
        let target = TabMenuTarget::capture(&before.core_state, 1, 1);
        let after = fixture(vec![E::TabMoved {
            id: 1,
            pane: 1,
            index: 2,
        }]);
        assert_eq!(target.resolve(&after.core_state), Some((1, 0)));
        assert_eq!(target.tab_id(), Some(4));
    }

    #[test]
    fn tab_target_is_gone_after_its_tab_closes() {
        let before = fixture(vec![]);
        let target = TabMenuTarget::capture(&before.core_state, 1, 0);
        let after = fixture(vec![E::TabClosed { id: 1 }]);
        assert_eq!(target.resolve(&after.core_state), None);
    }
}
