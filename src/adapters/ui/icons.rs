//! tasty-icons를 재노출하고 호스트에서 쓰는 별칭을 제공한다.
//! SVG 로더로 텍스처를 만들고 tint로 테마 색을 적용한다.

pub use tasty_icons::*;

pub use tasty_icons::{
    LAYOUT_DETAIL as DETAIL, LAYOUT_GRID as GRID, LIST as LIST_VIEW, MARKDOWN as MD,
    REMOTE as TERMINAL_PROMPT, TERMINAL as TERM,
};

/// 종류명이 아닌 매니페스트의 아이콘 이름으로 선택한다. 모르는 이름은 FILE로 표시한다.
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
        assert_eq!(from_name("markdown").uri, MD.uri);
        assert_eq!(from_name("folder").uri, FOLDER.uri);
        assert_eq!(from_name("image").uri, IMAGE.uri);
        assert_eq!(from_name("git_tree").uri, GIT_TREE.uri);
        assert_eq!(from_name("html").uri, HTML.uri);
        assert_eq!(from_name("terminal").uri, TERM.uri);
        assert_eq!(from_name("file").uri, FILE.uri);
        assert_eq!(from_name("no_such_icon").uri, FILE.uri);
    }
}
