//! 파일 선택 결과를 처리한다. 기본 동작은 로컬 파일을 열고 원격 경로를 클립보드에 복사한다.
//! 탭 생성 동작이면 지정한 pane에 탭을 만든다. 요청한 플러그인이 있으면 확정·취소 결과도 전달한다.

use crate::app::App;
use crate::app::command::DomainIntent;
use crate::app::window_access::engines_mut;
use crate::state::{FilePickerOnConfirm, FilePickerRequester, FilePickerResult, MainViewState};
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
            let core = &mut self.services;
            let plugin_manager = self.plugin_manager.as_mut();
            let Some((main, engine)) = engines_mut!(self).window_pair(id) else {
                continue;
            };
            let Some(data) = main.state.dialogs.file_picker.as_mut() else {
                continue;
            };
            let Some(result) = data.result.take() else {
                continue;
            };
            let requester = data.requester.clone();
            let on_confirm = data.on_confirm.clone();
            main.state.dialogs.file_picker = None;
            let copy_remote =
                dispatch_picker_result(&mut main.state, &on_confirm, &result, |pane_id| {
                    engine.has_pane(pane_id)
                });
            let (paths, cancelled) = match result {
                FilePickerResult::Cancelled => (Vec::new(), true),
                FilePickerResult::Confirmed { paths, .. } => (paths, false),
            };
            if copy_remote {
                apply_remote_confirm(core, &mut main.state, &paths);
            }
            if let Some(req) = requester {
                emit_file_picker_result(plugin_manager, &req, paths, cancelled);
            }
            main.mark_dirty();
        }
    }
}

/// 피커 결과를 처리할 intent를 넣는다. 원격 경로를 클립보드에 복사해야 하면 true를 돌려준다.
/// 취소는 아무것도 넣지 않는다. `has_pane`은 탭을 만들 pane이 아직 있는지 확인한다.
pub(crate) fn dispatch_picker_result(
    state: &mut MainViewState,
    on_confirm: &FilePickerOnConfirm,
    result: &FilePickerResult,
    has_pane: impl Fn(u32) -> bool,
) -> bool {
    let FilePickerResult::Confirmed { paths, is_remote } = result else {
        return false;
    };
    match on_confirm {
        FilePickerOnConfirm::OpenFile => {
            if *is_remote {
                return true;
            }
            for path in paths {
                state.dispatch_intent(
                    DomainIntent::DispatchFile {
                        target: crate::file::format::FileTarget::new(path.clone()),
                        depth: crate::file::format::DetectDepth::Deep,
                        origin_surface_id: None,
                        dispatch_origin: crate::file::dispatch::FileDispatchOrigin::User,
                        ignore_size_limit: false,
                    }
                    .from_user_menu("file_picker_confirm"),
                );
            }
        }
        FilePickerOnConfirm::NewTab {
            pane_id,
            kind,
            param_key,
        } => {
            // mirror pane에서는 피커를 열지 않으므로 원격 경로는 여기 오지 않는다.
            if *is_remote {
                tracing::warn!("file_picker: new {kind} tab ignores remote paths");
                return false;
            }
            // 피커가 열린 동안 pane이 닫혔을 수 있다.
            if !has_pane(*pane_id) {
                return false;
            }
            for path in paths {
                state.dispatch_intent(
                    DomainIntent::CreateTab {
                        pane_id: *pane_id,
                        cwd: None,
                        kind: kind.clone(),
                        name: None,
                        surface_params: serde_json::json!({ param_key.as_str(): path }),
                        activate: true,
                    }
                    .from_user_context_menu(),
                );
            }
        }
    }
    false
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
    core: &crate::app::services::AppServices,
    state: &mut crate::state::MainViewState,
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

#[cfg(test)]
#[path = "file_picker_tests.rs"]
mod tests;
