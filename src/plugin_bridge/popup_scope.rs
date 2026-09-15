//! Host-owned scope resolution shared by plugin shells and their file picker children.

use crate::adapters::ui::popup::{PopupScope, file_picker::FILE_PICKER_POPUP_ID};
use crate::state::AppState;
use tasty_host_plugin::manager::PopupInstance;
use tasty_plugin_manifest::PopupScopeDecl;

/// 매니페스트 선언과 host 가 바인딩한 대상으로 인스턴스의 소속 범위를 정한다.
///
/// `surface` 를 선언했어도 바인딩된 대상이 없으면(plugin 이 IPC·이벤트로 스스로 연 popup)
/// `Window` 다 — plugin 이 남의 surface 를 지목할 길을 열지 않으려고 대상은 host 진입점만
/// 채운다.
pub(crate) fn popup_scope(decl: PopupScopeDecl, scope_surface: Option<u32>) -> PopupScope {
    match (decl, scope_surface) {
        (PopupScopeDecl::Surface, Some(sid)) => PopupScope::Surface(sid),
        (PopupScopeDecl::Surface, None) | (PopupScopeDecl::Window, _) => PopupScope::Window,
    }
}

/// Resolve before host paint/hit testing, including the first frame after trigger.
/// Only the requesting plugin's own instance can supply the inherited scope.
/// This changes presentation alone: drafts, selections and request IDs stay intact.
pub(crate) fn inherit_file_picker_scope<'a>(
    state: &mut AppState,
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
        .map(|(_, inst)| popup_scope(inst.contribute.scope, inst.scope_surface))
        .next()
        .unwrap_or(PopupScope::Window);
    if let Some(popup) = state.popups.get_mut(FILE_PICKER_POPUP_ID) {
        popup.scope = scope;
    }
}
