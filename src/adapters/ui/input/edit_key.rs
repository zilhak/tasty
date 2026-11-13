//! 복사·잘라내기·붙여넣기 키의 공통 바인딩 판정. 한 키가 여러 편집 바인딩에 걸리면 정한 순서로 하나만
//! 고른다. 호스트 입력칸·HTML 주소 띠·WebView 편집 키가 같은 순서를 쓴다.

use tasty_key_match::matches_any_binding;
use winit::keyboard::{Key, ModifiersState};

/// 공통 판정이 다루는 편집 동작.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EditKey {
    Copy,
    Cut,
    Paste,
}

impl EditKey {
    /// 한 키가 여러 편집 바인딩에 걸리면 이 순서로 하나만 고른다.
    pub(crate) const ORDER: [EditKey; 3] = [Self::Copy, Self::Cut, Self::Paste];

    /// 같은 동작의 `KeybindingSettings` 필드 ID.
    pub(crate) fn field_id(self) -> &'static str {
        match self {
            Self::Copy => "copy",
            Self::Cut => "cut",
            Self::Paste => "paste",
        }
    }
}

/// `KeybindingSettings`에서 키가 가리키는 편집 동작 하나. 순서는 [`EditKey::ORDER`]다.
pub(crate) fn edit_binding(
    kb: &crate::settings::KeybindingSettings,
    key: &Key,
    mods: ModifiersState,
) -> Option<EditKey> {
    first_bound(
        |edit| kb.get_bindings(edit.field_id()).unwrap_or(&[]),
        key,
        mods,
    )
}

fn first_bound<'a>(
    combos: impl Fn(EditKey) -> &'a [String],
    key: &Key,
    mods: ModifiersState,
) -> Option<EditKey> {
    EditKey::ORDER
        .into_iter()
        .find(|edit| matches_any_binding(combos(*edit), key, mods))
}

impl From<EditKey> for crate::webview::WebViewEditAction {
    fn from(edit: EditKey) -> Self {
        match edit {
            EditKey::Copy => Self::Copy,
            EditKey::Cut => Self::Cut,
            EditKey::Paste => Self::Paste,
        }
    }
}

#[cfg(test)]
#[path = "edit_key_tests.rs"]
mod tests;
