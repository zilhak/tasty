//! 목록 항목의 글리프와 이름 색. 세 보기(Grid·List·Detail)가 같은 규칙을 쓴다.
//! 숨김 항목은 이름과 글리프를 explorer-hidden-fg 로 그린다(불투명도로 흐리게 하지 않는다).
//! 선택한 항목의 이름은 숨김 여부와 관계없이 선택 색이다.

use tasty_type_appearance::theme::Theme;

use super::view::DirEntryInfo;
use super::view::hidden::is_hidden;
use crate::adapters::ui::icons::{self, Icon};

/// explorer-hidden-fg. 디자인 토큰이 Theme 에 등록되기 전까지 같은 값인 text-muted 를 쓴다.
fn hidden_fg(theme: &Theme) -> egui::Color32 {
    theme.text_muted().to_egui()
}

/// 엔트리의 아이콘 + glyph 색 (design GridCell/DetailRow/ExpListMini):
/// 폴더/파일 = text-muted, 이미지 파일 = IMAGE 아이콘 + accent-info, 숨김 항목 = explorer-hidden-fg.
/// 대상이 없는 링크는 숨김 여부와 관계없이 링크 경고 글리프와 색을 그대로 쓴다.
pub(super) fn entry_icon(theme: &Theme, e: &DirEntryInfo) -> (Icon, egui::Color32) {
    if let Some(broken) = super::link::broken_icon(theme, e) {
        return broken;
    }
    let (icon, color) = if e.is_dir {
        (icons::FOLDER, theme.text_muted().to_egui())
    } else if super::is_image_ext(&e.ext) {
        (icons::IMAGE, theme.accent_info().to_egui())
    } else {
        (icons::FILE, theme.text_muted().to_egui())
    };
    (
        icon,
        if is_hidden(e) {
            hidden_fg(theme)
        } else {
            color
        },
    )
}

/// 이름 색. 선택이면 text-primary, 숨김 항목이면 explorer-hidden-fg, 아니면 보기마다 정한 `rest`.
pub(super) fn name_fg(
    theme: &Theme,
    e: &DirEntryInfo,
    selected: bool,
    rest: egui::Color32,
) -> egui::Color32 {
    if selected {
        theme.text_primary().to_egui()
    } else {
        rest_fg(theme, e).unwrap_or(rest)
    }
}

/// 선택·호버가 아닐 때 보기의 기본색 대신 쓸 색. 숨김 항목만 있다.
pub(super) fn rest_fg(theme: &Theme, e: &DirEntryInfo) -> Option<egui::Color32> {
    is_hidden(e).then(|| hidden_fg(theme))
}
