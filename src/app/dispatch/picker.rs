//! 파일 핸들러 선택 결과를 처리한다.

use crate::app::App;
use crate::view::ui::View;

impl App {
    /// MainView의 결과만 처리하며 parked 상태는 순회하지 않는다.
    pub(crate) fn dispatch_pending_picker_results(&mut self) {
        let pending: Vec<winit::window::WindowId> = self
            .view
            .views
            .iter()
            .filter_map(|(id, w)| {
                let main = w.as_main()?;
                let data = main.state.dialogs.file_handler_picker.as_ref()?;
                data.result.as_ref().map(|_| *id)
            })
            .collect();
        for id in pending {
            let core = &mut self.core;
            let Some(main) = self.view.views.get_mut(&id).and_then(|w| w.as_main_mut()) else {
                continue;
            };
            let Some(data) = main.state.dialogs.file_handler_picker.as_mut() else {
                continue;
            };
            let Some(result) = data.result.take() else {
                continue;
            };
            let target = data.target.clone();
            let origin_surface_id = data.origin_surface_id;
            let dispatch_origin = data.dispatch_origin;
            let ignore_size_limit = data.ignore_size_limit;
            main.state.dialogs.file_handler_picker = None;
            // 설정 창 열기는 ActiveEventLoop가 필요해 App에서 처리한다.
            if matches!(result, crate::state::FileHandlerPickerResult::OpenSettings) {
                self.pending_settings_file_handler_tab = true;
                crate::shortcuts::send_app_event(&self.view.proxy, crate::AppEvent::OpenSettings);
            } else {
                crate::file::dispatch::apply_file_picker_result(
                    core,
                    &mut main.state,
                    &mut main.core_state,
                    target,
                    result,
                    origin_surface_id,
                    dispatch_origin,
                    ignore_size_limit,
                );
            }
            main.mark_dirty();
        }
    }
}
