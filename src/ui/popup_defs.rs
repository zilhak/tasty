//! 모든 popup의 `PopupDef` 목록. 새 popup을 추가하려면 이 파일에 한 항목만 추가.

use std::sync::OnceLock;

use super::popup::{PopupDef, PopupScope};

/// 프로세스 수명 내내 살아있는 정적 popup 정의 목록.
pub fn all_defs() -> &'static [PopupDef] {
    static DEFS: OnceLock<Vec<PopupDef>> = OnceLock::new();
    DEFS.get_or_init(|| {
        vec![
            PopupDef {
                id: "notifications",
                title_key: "notification_panel.window_title",
                default_size: egui::vec2(350.0, 400.0),
                sizer: None,
                default_scope: PopupScope::Window,
                close_on_outside_click: false,
                draw_fn: super::notification_popup::draw_notification_popup,
            },
            PopupDef {
                id: "convert_surface",
                title_key: "convert_popup.title",
                default_size: super::convert_popup::convert_popup_default_size(),
                sizer: None,
                default_scope: PopupScope::Window,
                close_on_outside_click: true,
                draw_fn: super::convert_popup::draw_convert_popup,
            },
            PopupDef {
                id: "markdown_open",
                title_key: "dialog.markdown.title",
                default_size: egui::vec2(360.0, 200.0),
                sizer: Some(super::file_open_popup::markdown_popup_sizer),
                default_scope: PopupScope::Window,
                close_on_outside_click: false,
                draw_fn: super::file_open_popup::draw_markdown_open_popup,
            },
            PopupDef {
                id: "html_open",
                title_key: "dialog.html.title",
                default_size: egui::vec2(360.0, 200.0),
                sizer: Some(super::file_open_popup::html_popup_sizer),
                default_scope: PopupScope::Window,
                close_on_outside_click: false,
                draw_fn: super::file_open_popup::draw_html_open_popup,
            },
            PopupDef {
                id: "bookmark_name",
                title_key: "explorer.bookmark_add",
                default_size: super::bookmark_popup::bookmark_popup_default_size(),
                sizer: None,
                default_scope: PopupScope::Window,
                close_on_outside_click: false,
                draw_fn: super::bookmark_popup::draw_bookmark_popup,
            },
        ]
    })
}

/// id로 정의 하나를 찾는다.
pub fn find(id: &str) -> Option<&'static PopupDef> {
    all_defs().iter().find(|d| d.id == id)
}
