//! Native file picker (04) popup 의 result 슬롯 드레인.
//!
//! popup draw_fn(`crate::adapters::ui::popup::file_picker`)이
//! `state.dialogs.file_picker.result` 에 채워둔 `FilePickerResult` 를 본
//! dispatcher 가 매 frame begin 에 검사. 로컬 확정은 기존 `DomainIntent::DispatchFile`
//! 로(explorer/markdown 오픈과 동일 경로), 원격 확정은 컨텐츠 fetch 가 스코프 밖이라
//! 경로를 클립보드에 복사 + toast 로 알린다(트리거 지점 결정과 함께 신규 ADR에 근거
//! 기록).
//!
//! `FilePickerData.requester` 가 `Some` 이면(ADR-0058 — `file_picker.trigger`
//! 로 이 popup 을 연 plugin) 위 기존 동작에 **더해** `"file_picker.result"` 이벤트를
//! 그 plugin 에 unicast 한다 — `emit_host_event_to_plugin` 은 `PluginManager`(`App`
//! 소유) 접근이 필요해, `file_picker.trigger` IPC 핸들러(`CoreState` 큐잉만 가능)가
//! 아니라 이 App 레벨 drain 이 담당한다(`git_viewer.query_result` 와 동형 위치).

use crate::app::App;
use crate::core::intent::DomainIntent;
use crate::state::{FilePickerRequester, FilePickerResult};
use crate::view::ui::View;

/// `file_picker.result` 이벤트 payload — ADR-0058 Decision 4 가 고정한 최소 wire
/// 필드(`request_id`/`paths`/`cancelled`). 확정도 취소도 항상 세 필드 전부를 채워
/// plugin 이 하나의 구조체로 역직렬화할 수 있게 한다(확정 시 `cancelled: false`).
const FILE_PICKER_RESULT_EVENT: &str = "file_picker.result";

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
            // 데이터 슬롯 즉시 해제 — 빠른 popup 재오픈 시에도 중복 처리 방지.
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

/// `requester` 에게 `"file_picker.result"` 를 owner-unicast 로 push. plugin 이 이미
/// 종료됐으면 `emit_host_event_to_plugin` 이 조용히 폐기한다(정상 — 결과를 받을
/// 대상이 없을 뿐 에러 아님).
///
/// 소유 popup(ADR-0082)이 명시됐는데 그 인스턴스가 이미 사라졌다면 결과가 버려질
/// 가능성이 높다 — 연쇄 정리(`app::dispatch::plugin_popup_events`)가 제대로 돌았다면
/// 나오지 않아야 하는 조합이라 **조용히 넘기지 않고 경고를 남긴다.** 이벤트 자체는
/// 그대로 보낸다 — ADR-0058 의 "모든 트리거는 정확히 하나의 결과를 받는다" 는 popup
/// 생사와 무관한 계약이고, plugin 이 popup 밖에서 상관관계를 유지하고 있을 수도 있다.
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
            "file_picker result for request {} arrives after its owner popup instance {}              is gone — the requesting plugin ({}) may drop it",
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
