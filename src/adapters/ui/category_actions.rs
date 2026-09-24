//! 확장 사이드바와 축소 레일이 같은 카테고리 편집 팝업을 열도록 공유하는 진입점.

use crate::adapters::ui::popup::confirm_delete_category::CONFIRM_DELETE_CATEGORY_POPUP_ID;
use crate::intent::{OpenPopupMode, UiIntent};
use crate::model::WorkspaceCategoryId;
use crate::state::{AppState, RenameTarget};

pub(crate) fn open_new_category_dialog(state: &mut AppState) {
    let target = RenameTarget::NewCategory;
    let scope = target.popup_scope();
    state.dialogs.rename = Some((target, String::new()));
    state.dispatch_intent(
        UiIntent::OpenPopup {
            id: "rename",
            mode: OpenPopupMode::WithScope(scope),
        }
        .from_user_menu("category/new"),
    );
}

pub(crate) fn open_rename_category_dialog(
    state: &mut AppState,
    engine: &crate::core::CoreState,
    cat_id: WorkspaceCategoryId,
) {
    let name = engine.category_name(cat_id).unwrap_or_default().to_string();
    let target = RenameTarget::CategoryName { cat_id };
    let scope = target.popup_scope();
    state.dialogs.rename = Some((target, name));
    state.dispatch_intent(
        UiIntent::OpenPopup {
            id: "rename",
            mode: OpenPopupMode::WithScope(scope),
        }
        .from_user_menu("category/rename"),
    );
}

pub(crate) fn open_delete_category_confirm(state: &mut AppState, cat_id: WorkspaceCategoryId) {
    state.dialogs.pending_category_delete = Some(cat_id);
    state.dispatch_intent(
        UiIntent::OpenPopup {
            id: CONFIRM_DELETE_CATEGORY_POPUP_ID,
            mode: OpenPopupMode::CenteredFocused,
        }
        .from_user_menu("category/delete"),
    );
}
