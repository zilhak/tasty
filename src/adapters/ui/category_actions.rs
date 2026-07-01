//! 카테고리 생성/이름변경/삭제 다이얼로그를 여는 공용 진입점.
//!
//! 확장 사이드바 컨텍스트 메뉴(`view/main/redraw.rs`)와 축소 레일 카테고리
//! 팝업(`popup/rail_category.rs`)이 같은 다이얼로그(생성/이름변경 = rename 팝업,
//! 삭제 = confirm_delete_category)를 열도록 배선을 한 곳에 모은다.

use crate::adapters::ui::popup::confirm_delete_category::CONFIRM_DELETE_CATEGORY_POPUP_ID;
use crate::intent::{OpenPopupMode, UiIntent};
use crate::model::WorkspaceCategoryId;
use crate::state::{AppState, RenameTarget};

/// 새 카테고리 생성 다이얼로그(rename 팝업, 빈 버퍼) 열기.
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

/// 카테고리 이름변경 다이얼로그(rename 팝업, 현재 이름 초기값) 열기.
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

/// 카테고리 삭제 confirm 다이얼로그 열기(대상 id 기록 + 중앙 모달).
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
