//! File handler picker popup 의 result 슬롯 드레인.
//!
//! popup draw_fn 이 `state.dialogs.file_handler_picker.result` 에 채워둔
//! `FileHandlerPickerResult` 를 본 dispatcher 가 매 frame begin 에 검사.
//! 채워져 있으면 슬롯 해제 + `Core::apply_file_picker_result` Method 호출.
//! Phase D D.3.C.G.3.c — 옛 `file_dispatch::consume_picker_result` 의 자리.

use crate::app::App;
use crate::view::ui::View;

impl App {
    /// 모든 main window 의 picker result 슬롯 drain. parked state 는 *focused
    /// 윈도우가 아니므로 popup 미오픈* 가정 — main window 만 순회.
    pub(crate) fn dispatch_pending_picker_results(&mut self) {
        // self.core 와 self.view.views 동시 borrow 회피 — id 만 먼저 모은다.
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
            let ignore_size_limit = data.ignore_size_limit;
            // 데이터 슬롯 즉시 해제 — 빠른 popup 재오픈 시에도 중복 처리 방지.
            main.state.dialogs.file_handler_picker = None;
            core.apply_file_picker_result(
                &mut main.state,
                &mut main.core_state,
                target,
                result,
                ignore_size_limit,
            );
            main.mark_dirty();
        }
    }

    /// 대용량 markdown 확인 팝업(`markdown_size_confirm`)의 결정 슬롯 drain.
    /// popup wrapper 가 `pending_md_open.result` 에 `Some(true/false)` 를 채우면
    /// frame begin 에 검사 — `true` 면 `Core::apply_pending_md_open` 으로 오픈 재개,
    /// `false` 면 폐기. picker result 드레인과 동일 패턴.
    pub(crate) fn dispatch_pending_md_open(&mut self) {
        let pending: Vec<winit::window::WindowId> = self
            .view
            .views
            .iter()
            .filter_map(|(id, w)| {
                let main = w.as_main()?;
                let data = main.state.dialogs.pending_md_open.as_ref()?;
                data.result.map(|_| *id)
            })
            .collect();
        for id in pending {
            let core = &mut self.core;
            let Some(main) = self.view.views.get_mut(&id).and_then(|w| w.as_main_mut()) else {
                continue;
            };
            let Some(data) = main.state.dialogs.pending_md_open.as_ref() else {
                continue;
            };
            let Some(open) = data.result else {
                continue;
            };
            // 슬롯 즉시 해제 — 중복 처리 방지.
            let Some(pending) = main.state.dialogs.pending_md_open.take() else {
                continue;
            };
            if open {
                core.apply_pending_md_open(&mut main.state, &mut main.core_state, pending);
            }
            main.mark_dirty();
        }
    }
}
