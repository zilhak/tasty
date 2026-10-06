//! 플러그인 팝업과 자식 파일 피커의 소속 범위를 호스트가 정한다.

use crate::adapters::ui::popup::{PopupScope, file_picker::FILE_PICKER_POPUP_ID};
use crate::state::RequestContext;
use tasty_host_plugin::manager::PopupInstance;
use tasty_plugin_manifest::PopupScopeDecl;

/// surface 선언에 바인딩된 대상이 없으면 Window로 처리한다.
/// 플러그인이 다른 surface를 지정하지 못하도록 대상은 호스트 진입점에서만 채운다.
pub(crate) fn popup_scope(decl: PopupScopeDecl, scope_surface: Option<u32>) -> PopupScope {
    match (decl, scope_surface) {
        (PopupScopeDecl::Surface, Some(sid)) => PopupScope::Surface(sid),
        (PopupScopeDecl::Surface, None) | (PopupScopeDecl::Window, _) => PopupScope::Window,
    }
}

/// 첫 프레임을 그리기 전에 요청한 플러그인의 팝업에서 범위를 상속한다.
/// 부모가 직전 프레임에 실제로 놓인 범위가 있으면 그것을 따른다(좁은 surface 라 창 범위로
/// 연 부모). 없으면(아직 안 그렸거나 숨음) 선언 종류와 대상으로 정한다.
/// 화면 배치만 바꾸며 입력 초안·선택·요청 ID는 유지한다.
pub(crate) fn inherit_file_picker_scope<'a>(
    state: &mut RequestContext,
    instances: impl Iterator<Item = (u64, &'a PopupInstance)>,
) {
    let Some(data) = state.dialogs.file_picker.as_ref() else {
        return;
    };
    let owner = data
        .requester
        .as_ref()
        .and_then(|r| r.owner_popup_instance.map(|id| (id, r.plugin_id.as_str())));
    let scope = instances
        .filter(|(id, inst)| owner == Some((*id, inst.plugin_id.as_str())))
        .map(|(id, inst)| {
            state
                .plugin_popup_placed_scopes
                .get(&id)
                .cloned()
                .unwrap_or_else(|| popup_scope(inst.contribute.scope, inst.scope_surface))
        })
        .next()
        .unwrap_or(PopupScope::Window);
    if let Some(popup) = state.popups.get_mut(FILE_PICKER_POPUP_ID) {
        popup.scope = scope;
    }
}
