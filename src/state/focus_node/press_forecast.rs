//! native 누름 알림 한 건이 지금 상태에 적용됐을 때의 결과표(`webview::PressForecast`). 결과는 알림 소비의 전이
//! (`MainViewState::press_surface_body`, 포커스 획득은 `focus_surface_by_id` 뒤 `normalize_focus`)와 같은 판정을 읽는다:
//! 창 전체를 막는 노드(무대, 설정 열기 요청, 키를 가진 복원 안내), 표시된 탭, 그 surface 경로를 덮는 덮개
//! (`top_cover_over`), 지금 포커스 노드.
//! TODO: S6c 입력 순서 경계 설계가 대체할 임시 장치.

use super::FocusNode;
use crate::adapters::ui::popup::PopupScope;
use crate::adapters::ui::popup::route::path_of_scope;
use crate::core::CoreState;
use crate::state::MainViewState;
use crate::webview::PressForecast;

impl MainViewState {
    /// 지금 상태에서 surface 마다 누름·포커스 획득 알림 한 건의 결과. 게시 자리(`key_release::publish_key_holder`)가
    /// 정규화 뒤에 부른다.
    pub(crate) fn press_forecast(&self, core: &CoreState) -> PressForecast {
        let selected = self.focused_surface_id(core);
        let mut forecast = PressForecast {
            selected,
            ..Default::default()
        };
        if core.workspaces().is_empty() {
            return forecast;
        }
        if self.fullscreen_stage_active()
            || self.has_settings_open_request()
            || crate::adapters::ui::restore_notice::has_keyboard_focus(&self.popups)
        {
            forecast.window_blocked = true;
            return forecast;
        }
        let node = self.focus_node(core);
        forecast.free = matches!(node, FocusNode::Surface(_));
        let ws = self.active_workspace(core);
        for pane_id in ws.pane_layout().all_pane_ids() {
            let Some(pane) = ws.pane_layout().find_pane(pane_id) else {
                continue;
            };
            let Some(tab) = pane.tabs.get(self.navigation.tab_index(pane)) else {
                continue;
            };
            // 아직 만들지 않은 탭(지연 생성)에는 누를 surface 가 없다.
            let sids = tab.layout_opt.as_ref().map(|l| l.all_surface_ids());
            for sid in sids.into_iter().flatten() {
                let path = path_of_scope(core, &PopupScope::Surface(sid));
                if let Some(cover) = self.top_cover_over(&path, None) {
                    forecast.covered.insert(sid);
                    // 누름은 덮개를 누른 것과 같다(`apply_press_target`). 포커스를 받지 않는 덮개면 아무것도 바꾸지 않는다.
                    if self.popups.press_takes_focus(cover) {
                        forecast.click_covered.insert(sid);
                    }
                    continue;
                }
                forecast.click_holds.insert(sid);
                #[cfg(any(windows, target_os = "macos", test))]
                {
                    forecast.focus_valid.insert(sid);
                    // 포커스 획득은 팝업만 놓을 수 있다: 선택이 바뀌고 그 팝업 범위가 새 경로 밖일 때다(`normalize_focus`).
                    if let FocusNode::Popup(popup) = node
                        && selected != Some(sid)
                        && self
                            .node_logical_scope(popup)
                            .is_some_and(|scope| !path.contains(&scope))
                    {
                        forecast.focus_releases_now.insert(sid);
                    }
                }
            }
        }
        forecast
    }
}
