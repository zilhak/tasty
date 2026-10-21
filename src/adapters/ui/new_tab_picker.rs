//! 탭 스트립 메뉴의 "새 HTML..."처럼 고른 파일로 탭을 만드는 호출자. 피커가 돌려준 경로로
//! 메뉴를 연 pane에 탭을 만든다.

use crate::adapters::ui::popup::file_picker;
use crate::app::command::DomainIntent;
use crate::core::CoreState;
use crate::runtime::engine_read::EngineRead;
use crate::state::{
    FilePickerLocation, FilePickerOutcome, FilePickerRequestId, MainViewState, NewTabPickerRequest,
};

/// 고른 파일로 `pane_id` pane에 `kind` 탭을 만드는 피커를 연다. 메뉴를 연 pane의
/// 선택 탭 cwd에서 출발한다. mirror pane이면 피커를 열지 않고 토스트를 띄운다.
/// html 같은 kind는 mirror에서 내용을 볼 수 없어 원격 파일 열기 거절과 같게 다룬다.
pub(crate) fn open(
    state: &mut MainViewState,
    engine: &EngineRead<'_>,
    pane_id: u32,
    kind: &str,
    param_key: &str,
    extensions: &[&str],
) {
    let is_mirror = engine
        .find_workspace_index_for_pane(pane_id)
        .and_then(|idx| engine.workspace_at(idx))
        .is_some_and(|ws| ws.mirror);
    if is_mirror {
        state.toasts.push(
            crate::i18n::t("explorer.state.remote_open_unsupported").to_string(),
            crate::adapters::ui::ToastKind::Info,
            crate::adapters::ui::ToastScope::Window,
        );
        return;
    }
    let dir = engine
        .find_pane_by_id(pane_id)
        .and_then(|pane| pane.tabs.get(state.navigation.tab_index(pane)))
        .and_then(|tab| state.navigation.surface_id(tab))
        .and_then(|sid| engine.surface_cwd(sid))
        .and_then(|cwd| match cwd {
            crate::core::state::SurfaceCwd::Local(p) => Some(p.to_string_lossy().into_owned()),
            crate::core::state::SurfaceCwd::Remote(_) => None,
        });
    let request_id = FilePickerRequestId::next();
    state.dialogs.file_picker_callers.new_tab = Some(NewTabPickerRequest {
        request_id,
        pane_id,
        kind: kind.to_string(),
        param_key: param_key.to_string(),
    });
    file_picker::open(
        state,
        engine,
        request_id,
        FilePickerLocation::Local { dir },
        extensions.iter().map(|e| e.to_string()).collect(),
        file_picker::FilePickerOpenedBy::ContextMenu,
    );
}

/// 보관한 요청과 같은 식별자의 결과만 한 번 적용한다. 경로마다 탭을 하나씩 만든다.
/// 취소했거나 피커가 열린 동안 pane이 닫혔으면 아무것도 만들지 않는다.
pub(crate) fn apply_outcome(
    state: &mut MainViewState,
    engine: &CoreState,
    outcome: &FilePickerOutcome,
) {
    let callers = &mut state.dialogs.file_picker_callers;
    if callers.new_tab.as_ref().map(|r| r.request_id) != Some(outcome.request_id) {
        return;
    }
    let Some(request) = callers.new_tab.take() else {
        return;
    };
    if outcome.cancelled || !engine.has_pane(request.pane_id) {
        return;
    }
    for path in &outcome.paths {
        state.dispatch_intent(
            DomainIntent::CreateTab {
                pane_id: request.pane_id,
                cwd: None,
                kind: request.kind.clone(),
                name: None,
                surface_params: serde_json::json!({ request.param_key.as_str(): path }),
                activate: true,
            }
            .from_user_context_menu(),
        );
    }
}

#[cfg(test)]
#[path = "new_tab_picker_tests.rs"]
mod tests;
