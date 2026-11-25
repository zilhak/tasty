//! 폴링으로 surface·워크스페이스·탭 포커스와 pane 간 탭 이동을 감지한다.

use super::{PendingHostEvent, RequestContext};
use crate::core::CoreState;

impl RequestContext {
    /// plugin `surface.focused`·`surface.unfocused`로 갈 이벤트를 일어난 순서대로 낸다
    /// (`MainViewState::take_key_focus_moves`). `surface.focused`는 화면에 낸 프레임에서 host 키 포커스가 surface로
    /// 들어갈 때마다 하나다. 키를 가진 팝업이 닫혀 같은 surface로 돌아오면 앞 값 없이 다시 낸다.
    /// `surface.unfocused`는 키 포커스가 plugin surface(`is_plugin_surface`)를 떠날 때 그 surface에 하나이고, 다른
    /// surface로 옮겨 가면 그 surface의 `surface.focused`보다 먼저다.
    pub fn detect_focus_change(
        &mut self,
        is_plugin_surface: impl Fn(u32) -> bool,
    ) -> Vec<PendingHostEvent> {
        use crate::state::key_focus_exits::KeyFocusMove;
        self.take_key_focus_moves()
            .into_iter()
            .filter_map(|m| match m {
                KeyFocusMove::Entered { surface_id, prev } => {
                    Some(PendingHostEvent::SurfaceFocused {
                        surface_id,
                        prev_surface_id: prev,
                    })
                }
                KeyFocusMove::Left { surface_id, next } => {
                    is_plugin_surface(surface_id).then_some(PendingHostEvent::SurfaceUnfocused {
                        surface_id,
                        next_surface_id: next,
                    })
                }
            })
            .collect()
    }

    /// 선택 surface가 바뀌었으면 새 선택 surface. 탭 제목의 OSC 제목을 그 surface 것으로 다시 읽는 데 쓴다.
    /// 키 포커스와 무관하게 선택을 따른다(팝업 아래에서 선택이 바뀌어도 제목은 바뀐다).
    pub fn detect_selection_change(&mut self, engine: &CoreState) -> Option<u32> {
        let current = self.focused_surface_id(engine);
        if current == self.last_focused_surface_id {
            return None;
        }
        self.last_focused_surface_id = current;
        current
    }

    pub fn detect_workspace_activation(&mut self, engine: &CoreState) -> Vec<PendingHostEvent> {
        let mut events = Vec::new();
        let current = engine
            .workspace_at(self.active_workspace_index(engine))
            .map(|w| w.id);
        if current == self.last_active_workspace_id {
            return events;
        }
        let prev = self.last_active_workspace_id;
        self.last_active_workspace_id = current;
        if let Some(workspace_id) = current {
            events.push(PendingHostEvent::WorkspaceActivated {
                workspace_id,
                prev_workspace_id: prev,
            });
        }
        events
    }

    pub fn detect_tab_focus_change(&mut self, engine: &CoreState) -> Vec<PendingHostEvent> {
        let mut events = Vec::new();
        let current = self.focused_pane(engine).and_then(|pane| {
            pane.tabs
                .get(self.navigation.tab_index(pane))
                .map(|tab| (pane.id, tab.id))
        });
        if current == self.last_focused_tab {
            return events;
        }
        let prev_tab_id = self.last_focused_tab.map(|(_, tab_id)| tab_id);
        self.last_focused_tab = current;
        if let Some((pane_id, tab_id)) = current {
            events.push(PendingHostEvent::TabFocused {
                tab_id,
                pane_id,
                prev_tab_id,
            });
        }
        events
    }
}
