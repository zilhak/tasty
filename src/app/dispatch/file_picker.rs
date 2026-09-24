//! 파일 선택 결과를 처리한다. 로컬 파일은 열고 원격 경로는 클립보드에 복사한다.
//! 요청한 플러그인이 있으면 확정·취소 결과도 전달한다.

use crate::app::App;
use crate::core::intent::DomainIntent;
use crate::state::{FilePickerRequester, FilePickerResult};
use crate::view::ui::View;

/// 확정과 취소 모두 request_id·paths·cancelled를 포함한다.
const FILE_PICKER_RESULT_EVENT: &str = "file_picker.result";

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
            let core = &mut self.core;
            let plugin_manager = self.plugin_manager.as_mut();
            let Some(main) = self.view.views.get_mut(&id).and_then(|w| w.as_main_mut()) else {
                continue;
            };
            let Some(data) = main.state.dialogs.file_picker.as_mut() else {
                continue;
            };
            let Some(result) = data.result.take() else {
                continue;
            };
            let requester = data.requester.clone();
            main.state.dialogs.file_picker = None;
            match result {
                FilePickerResult::Cancelled => {
                    if let Some(req) = requester {
                        emit_file_picker_result(plugin_manager, &req, Vec::new(), true);
                    }
                }
                FilePickerResult::Confirmed { paths, is_remote } => {
                    if is_remote {
                        apply_remote_confirm(core, &mut main.state, &paths);
                    } else {
                        for path in &paths {
                            main.state.dispatch_intent(
                                DomainIntent::DispatchFile {
                                    target: crate::file::format::FileTarget::new(path.clone()),
                                    depth: crate::file::format::DetectDepth::Deep,
                                    origin_surface_id: None,
                                    dispatch_origin:
                                        crate::file::dispatch::FileDispatchOrigin::User,
                                    ignore_size_limit: false,
                                }
                                .from_user_menu("file_picker_confirm"),
                            );
                        }
                    }
                    if let Some(req) = requester {
                        emit_file_picker_result(plugin_manager, &req, paths, false);
                    }
                }
            }
            main.mark_dirty();
        }
    }
}

/// 소유 popup이 사라져도 요청한 플러그인에 결과를 보낸다.
/// 플러그인이 popup 밖에서 요청 상태를 관리할 수도 있기 때문이다.
fn emit_file_picker_result(
    plugin_manager: Option<&mut crate::plugin::PluginManager>,
    requester: &FilePickerRequester,
    paths: Vec<String>,
    cancelled: bool,
) {
    let Some(mgr) = plugin_manager else {
        return;
    };
    if let Some(owner) = requester.owner_popup_instance
        && !mgr.popup_instances().any(|(id, _)| id == owner)
    {
        tracing::warn!(
            "file_picker request {} completed after owner popup {} closed; plugin {} may ignore the result",
            requester.request_id,
            owner,
            requester.plugin_id
        );
    }
    mgr.emit_host_event_to_plugin(
        &requester.plugin_id,
        FILE_PICKER_RESULT_EVENT,
        &serde_json::json!({
            "request_id": requester.request_id,
            "paths": paths,
            "cancelled": cancelled,
        }),
        tasty_plugin_protocol::EventScope::System,
    );
}

/// 원격 파일을 내려받지 않고 선택한 경로만 복사한다.
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
