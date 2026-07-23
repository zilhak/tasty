//! Native file picker (04) popup 의 result 슬롯 드레인.
//!
//! popup draw_fn(`crate::adapters::ui::popup::file_picker`)이
//! `state.dialogs.file_picker.result` 에 채워둔 `FilePickerResult` 를 본
//! dispatcher 가 매 frame begin 에 검사. 로컬 확정은 기존 `DomainIntent::DispatchFile`
//! 로(explorer/markdown 오픈과 동일 경로), 원격 확정은 컨텐츠 fetch 가 스코프 밖이라
//! 경로를 클립보드에 복사 + toast 로 알린다(트리거 지점 결정과 함께 신규 ADR에 근거
//! 기록).

use crate::app::App;
use crate::core::intent::DomainIntent;
use crate::state::FilePickerResult;
use crate::view::ui::View;

impl App {
    /// 모든 main window 의 file_picker result 슬롯 drain. parked state 는 *focused
    /// 윈도우가 아니므로 popup 미오픈* 가정 — main window 만 순회.
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
            let core = &mut self.core;
            let Some(main) = self.view.views.get_mut(&id).and_then(|w| w.as_main_mut()) else {
                continue;
            };
            let Some(data) = main.state.dialogs.file_picker.as_mut() else {
                continue;
            };
            let Some(result) = data.result.take() else {
                continue;
            };
            // 데이터 슬롯 즉시 해제 — 빠른 popup 재오픈 시에도 중복 처리 방지.
            main.state.dialogs.file_picker = None;
            match result {
                FilePickerResult::Cancelled => {}
                FilePickerResult::Confirmed { paths, is_remote } => {
                    if is_remote {
                        apply_remote_confirm(core, &mut main.state, &paths);
                    } else {
                        for path in paths {
                            main.state.dispatch_intent(
                                DomainIntent::DispatchFile {
                                    target: crate::file::format::FileTarget::new(path),
                                    depth: crate::file::format::DetectDepth::Deep,
                                    origin_surface_id: None,
                                    ignore_size_limit: false,
                                }
                                .from_user_menu("file_picker_confirm"),
                            );
                        }
                    }
                }
            }
            main.mark_dirty();
        }
    }
}

/// 원격 확정 — 컨텐츠를 이 세션으로 가져오는 것은 이번 구현 스코프 밖(디렉토리
/// 나열만 설계됨)이라, 선택 경로를 클립보드에 복사하고 toast 로 알린다.
fn apply_remote_confirm(
    core: &crate::core::Core,
    state: &mut crate::state::AppState,
    paths: &[String],
) {
    let joined = paths.join("\n");
    let message = if let Err(e) = core.clipboard_arc().write_text(&joined) {
        tracing::warn!("file_picker 원격 경로 클립보드 복사 실패: {e}");
        crate::i18n::t("filepicker.remote_confirm_clipboard_failed").to_string()
    } else {
        crate::i18n::t("filepicker.remote_confirm_copied").to_string()
    };
    state
        .toasts
        .push_info(message, crate::model::toast_kind::ToastScope::Window);
}
