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
                title_fn: None,
                default_size: egui::vec2(350.0, 400.0),
                sizer: None,
                default_scope: PopupScope::Window,
                close_on_outside_click: false,
                headless: false,
                sticky_focus: false,
                draw_fn: super::notification_popup::draw_notification_popup,
            },
            PopupDef {
                id: "convert_surface",
                title_key: "convert_popup.title",
                title_fn: None,
                default_size: super::convert_popup::convert_popup_default_size(),
                sizer: Some(super::convert_popup::convert_popup_sizer),
                default_scope: PopupScope::Window,
                close_on_outside_click: true,
                headless: false,
                sticky_focus: false,
                draw_fn: super::convert_popup::draw_convert_popup,
            },
            PopupDef {
                id: "markdown_open",
                title_key: "dialog.markdown.title",
                title_fn: None,
                default_size: egui::vec2(360.0, 200.0),
                sizer: Some(super::file_open_popup::markdown_popup_sizer),
                default_scope: PopupScope::Window,
                close_on_outside_click: false,
                headless: false,
                sticky_focus: false,
                draw_fn: super::file_open_popup::draw_markdown_open_popup,
            },
            PopupDef {
                id: "html_open",
                title_key: "dialog.html.title",
                title_fn: None,
                default_size: egui::vec2(360.0, 200.0),
                sizer: Some(super::file_open_popup::html_popup_sizer),
                default_scope: PopupScope::Window,
                close_on_outside_click: false,
                headless: false,
                sticky_focus: false,
                draw_fn: super::file_open_popup::draw_html_open_popup,
            },
            PopupDef {
                id: "rename",
                title_key: "rename_dialog.tab_heading",
                title_fn: Some(super::dialog::rename_popup_title),
                default_size: super::dialog::rename_popup_default_size(),
                sizer: None,
                default_scope: PopupScope::Window,
                close_on_outside_click: false,
                headless: false,
                sticky_focus: false,
                draw_fn: super::dialog::draw_rename_popup,
            },
            PopupDef {
                id: "search_bar",
                title_key: "search.placeholder",
                title_fn: None,
                default_size: egui::vec2(360.0, 28.0),
                sizer: None,
                default_scope: PopupScope::Window,
                close_on_outside_click: false,
                headless: true,
                sticky_focus: true,
                draw_fn: super::search_bar::draw_search_bar,
            },
            PopupDef {
                id: super::info_modal::INFO_MODAL_ID,
                title_key: "button.ok",
                title_fn: Some(super::info_modal::info_modal_title),
                default_size: egui::vec2(440.0, 160.0),
                sizer: Some(super::info_modal::info_modal_sizer),
                default_scope: PopupScope::Window,
                close_on_outside_click: false,
                headless: false,
                sticky_focus: false,
                draw_fn: super::info_modal::draw_info_modal,
            },
            PopupDef {
                id: "tools_menu",
                title_key: "tools_menu.title",
                title_fn: None,
                default_size: egui::vec2(160.0, 36.0),
                sizer: None,
                default_scope: PopupScope::Window,
                close_on_outside_click: true,
                headless: true,
                sticky_focus: false,
                draw_fn: super::tools_menu::draw_tools_menu,
            },
        ]
    })
}

/// id로 정의 하나를 찾는다.
pub fn find(id: &str) -> Option<&'static PopupDef> {
    all_defs().iter().find(|d| d.id == id)
}
