//! ui_kit line-icon set — canonical 소스는 [`tasty_icons`] 크레이트.
//!
//! 이 모듈은 크레이트 아이콘을 재노출하고 host 로컬 이름 별칭만 유지한다(중복 path
//! 정의 제거). egui_extras 의 svg 로더(`gpu.rs` 의 `install_image_loaders`)가
//! 텍스처화하고, `Icon::image` 의 `tint` 로 테마 색을 입힌다.

pub use tasty_icons::*;

// host 로컬 이름 → canonical 별칭 (기존 사용처 무변경 목적).
pub use tasty_icons::{
    LAYOUT_DETAIL as DETAIL, LAYOUT_GRID as GRID, LIST as LIST_VIEW, MARKDOWN as MD,
    REMOTE as TERMINAL_PROMPT, TERMINAL as TERM,
};

/// surface kind 등 매니페스트/registry 가 선언한 아이콘 **이름** → glyph 매핑.
/// host 가 kind 를 이름으로 분기(`match kind { "markdown" => MD }`)하지 않고, kind 가
/// 선언한 icon 이름을 이 host-소유 아이콘 세트에서 해석한다. 미지정/미지의 이름은
/// 중립 `FILE` 로 떨어진다(plugin/remote kind 안전망).
pub fn from_name(name: &str) -> Icon {
    match name {
        "markdown" => MD,
        "folder" => FOLDER,
        "image" => IMAGE,
        "html" => HTML,
        "terminal" => TERM,
        "git_tree" => GIT_TREE,
        "file" => FILE,
        _ => FILE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_name_maps_surface_kind_icons() {
        // uri 로 아이콘 동일성 비교(Icon 는 PartialEq 미구현).
        assert_eq!(from_name("markdown").uri, MD.uri);
        assert_eq!(from_name("folder").uri, FOLDER.uri);
        assert_eq!(from_name("image").uri, IMAGE.uri);
        assert_eq!(from_name("git_tree").uri, GIT_TREE.uri);
        assert_eq!(from_name("html").uri, HTML.uri);
        assert_eq!(from_name("terminal").uri, TERM.uri);
        assert_eq!(from_name("file").uri, FILE.uri);
        // 미지의 이름은 중립 FILE 로 fallback.
        assert_eq!(from_name("no_such_icon").uri, FILE.uri);
    }
}
