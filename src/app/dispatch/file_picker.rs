//! 파일 피커가 남긴 결과를 꺼내 피커를 연 호출자에게 넘긴다. 피커와 이 전달 경로는
//! 파일을 열거나 탭을 만들거나 클립보드에 쓰지 않는다. 결과로 무엇을 할지는 각 호출자가 정한다.

use crate::app::App;
use crate::app::window_access::engines_mut;
use crate::core::CoreState;
use crate::state::{FilePickerOutcome, FilePickerRequester, MainViewState};
use crate::view::ui::View;

impl App {
    /// 현재 MainView의 결과만 처리하며 parked 상태는 순회하지 않는다.
    pub(crate) fn dispatch_pending_file_picker_results(&mut self) {
        let pending: Vec<winit::window::WindowId> = self
            .view
            .views
            .iter()
            .filter_map(|(id, w)| {
                let main = w.as_main()?;
                let data = main.state.dialogs.file_picker.as_ref()?;
                data.result.as_ref().map(|_| *id)
            })
            .collect();
        for id in pending {
            let plugin_manager = self.plugin_manager.as_mut();
            let Some((main, engine)) = engines_mut!(self).window_pair(id) else {
                continue;
            };
            let Some((outcome, requester)) = take_outcome(&mut main.state) else {
                continue;
            };
            deliver_outcome(
                &mut main.state,
                &engine,
                plugin_manager,
                requester.as_ref(),
                &outcome,
            );
            main.mark_dirty();
        }
    }
}

/// 닫힌 피커의 결과를 한 번 꺼내고 피커 상태를 지운다. 결과가 없으면 피커를 그대로 둔다.
pub(crate) fn take_outcome(
    state: &mut MainViewState,
) -> Option<(FilePickerOutcome, Option<FilePickerRequester>)> {
    let result = state.dialogs.file_picker.as_mut()?.result.take()?;
    let data = state.dialogs.file_picker.take()?;
    Some((
        FilePickerOutcome::new(data.request_id, result),
        data.requester,
    ))
}

/// 결과를 호출자에게 넘긴다. 본체 호출자는 자기가 보관한 식별자와 같을 때만 적용하고,
/// 플러그인 요청이면 그 플러그인에 `file_picker.result`를 보낸다.
pub(crate) fn deliver_outcome(
    state: &mut MainViewState,
    engine: &CoreState,
    plugin_manager: Option<&mut crate::plugin::PluginManager>,
    requester: Option<&FilePickerRequester>,
    outcome: &FilePickerOutcome,
) {
    crate::adapters::ui::file_open_tool::apply_outcome(state, engine, outcome);
    crate::adapters::ui::new_tab_picker::apply_outcome(state, engine, outcome);
    if let Some(req) = requester {
        crate::adapters::ipc::handler::file_picker::deliver_result(plugin_manager, req, outcome);
    }
}

#[cfg(test)]
#[path = "file_picker_tests.rs"]
mod tests;
