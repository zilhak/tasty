//! 목록의 링크 표시. 대상이 있는 링크는 대상의 글리프를 쓰고 이름 뒤에 작은 link 글리프를 붙인다.
//! 대상이 없는 링크는 빌릴 글리프가 없어 항목 자리에 link 글리프를 warning 색으로 그리고,
//! 이름은 보통 색이다(지워진 것이 없으므로 취소선도 없다). 호버하면 없는 대상의 경로를 보인다.
//! Detail · List · Grid 가 같은 표시를 쓴다.

use tasty_type_appearance::theme::Theme;

use crate::adapters::ui::icons::{self, Icon};
use crate::core::fs_list::{DirEntryInfo, EntryLink};
use crate::i18n::t;

/// 이름 뒤 link 글리프의 크기.
pub(super) fn glyph_size(th: &Theme) -> f32 {
    th.explorer_link_glyph_size().value()
}

fn glyph_color(th: &Theme) -> egui::Color32 {
    th.explorer_link_glyph().to_egui()
}

/// 대상이 없는 링크의 항목 글리프와 색. 링크가 아니거나 대상이 있으면 `None`.
pub(super) fn broken_icon(th: &Theme, e: &DirEntryInfo) -> Option<(Icon, egui::Color32)> {
    (e.link == EntryLink::Broken).then(|| (icons::LINK, th.explorer_link_broken_fg().to_egui()))
}

/// 이름 뒤에 link 글리프가 붙는 항목인지.
pub(super) fn has_tail(e: &DirEntryInfo) -> bool {
    e.link == EntryLink::Valid
}

/// 글자 끝 `text_right` 에서 `space-xs` 띄운 자리에 link 글리프를 그린다. `fade` 는 잘라내기 디밍 같은 색 보정이다.
pub(super) fn paint_tail(
    ui: &egui::Ui,
    th: &Theme,
    text_right: f32,
    center_y: f32,
    fade: impl Fn(egui::Color32) -> egui::Color32,
) {
    let s = glyph_size(th);
    let rect = egui::Rect::from_min_size(
        egui::pos2(text_right + th.spacing_xs.value(), center_y - s / 2.0),
        egui::vec2(s, s),
    );
    icons::LINK
        .image(s, fade(glyph_color(th)))
        .paint_at(ui, rect);
}

/// 이름 칸에 link 글리프를 한 칸 더 잡아 그린다(Detail 의 가로 배치 안에서 쓴다).
pub(super) fn add_tail(
    ui: &mut egui::Ui,
    th: &Theme,
    fade: impl Fn(egui::Color32) -> egui::Color32,
) {
    let s = glyph_size(th);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(s, s), egui::Sense::hover());
    icons::LINK
        .image(s, fade(glyph_color(th)))
        .paint_at(ui, rect);
}

/// 대상이 없는 링크면 호버에 "Target not found: {path}" 를 보인다. 경로는 호버할 때만 읽는다.
pub(super) fn broken_tooltip(e: &DirEntryInfo, resp: egui::Response) -> egui::Response {
    if e.link != EntryLink::Broken || !resp.hovered() {
        return resp;
    }
    let target = std::fs::read_link(&e.path).map_or_else(
        |_| e.path.display().to_string(),
        |p| p.display().to_string(),
    );
    resp.on_hover_text(t("explorer.link.target_missing").replace("{path}", &target))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn entry(link: EntryLink) -> DirEntryInfo {
        DirEntryInfo {
            path: PathBuf::from("/srv/old-config.toml"),
            name: "old-config.toml".into(),
            is_dir: false,
            size: 0,
            modified: None,
            ext: "toml".into(),
            link,
        }
    }

    #[test]
    fn only_a_live_link_gets_the_trailing_glyph() {
        assert!(has_tail(&entry(EntryLink::Valid)));
        assert!(!has_tail(&entry(EntryLink::Broken)));
        assert!(!has_tail(&entry(EntryLink::NotALink)));
    }

    #[test]
    fn a_broken_link_takes_the_item_slot_in_warning() {
        let th = crate::theme::theme();
        let (icon, color) = broken_icon(&th, &entry(EntryLink::Broken)).expect("broken link");
        assert_eq!(icon.uri, icons::LINK.uri);
        assert_eq!(color, th.explorer_link_broken_fg().to_egui());
        assert!(broken_icon(&th, &entry(EntryLink::Valid)).is_none());
        assert!(broken_icon(&th, &entry(EntryLink::NotALink)).is_none());
    }

    #[test]
    fn the_tail_glyph_reads_the_link_tokens() {
        let th = crate::theme::theme();
        assert_eq!(glyph_size(&th), th.explorer_link_glyph_size().value());
        assert_eq!(glyph_color(&th), th.explorer_link_glyph().to_egui());
    }
}
