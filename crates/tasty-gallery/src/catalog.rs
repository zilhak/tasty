//! 카탈로그 항목 등록.
//!
//! 한 항목은 `(name, draw)` 의 페어. `draw` 는 선택 시 우측 디테일 패널에
//! 호출되는 함수로, `Theme` 을 받아 egui 위젯을 그린다.

pub mod components;
pub mod spacing;
pub mod theme;
pub mod typography;
pub mod widgets;

use tasty_type_appearance::theme::Theme;

/// 카탈로그 1차 분류. 상단 탭 + 좌측 사이드바 필터링에 사용.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    Appearance,
    Widget,
    Popup,
    Component,
    Layout,
}

impl Category {
    pub fn label(self) -> &'static str {
        match self {
            Category::Appearance => "Appearance",
            Category::Widget => "Widget",
            Category::Popup => "Popup",
            Category::Component => "Component",
            Category::Layout => "Layout",
        }
    }

    pub fn all() -> &'static [Category] {
        &[
            Category::Appearance,
            Category::Widget,
            Category::Popup,
            Category::Component,
            Category::Layout,
        ]
    }
}

/// 좌측 사이드바에 표시되는 카탈로그 한 항목.
#[derive(Clone, Copy)]
pub struct CatalogItem {
    pub category: Category,
    pub name: &'static str,
    pub draw: fn(&mut egui::Ui, &Theme),
}

/// 모든 카탈로그 항목. 좌측 트리는 이 목록을 순회한다.
pub fn all() -> Vec<CatalogItem> {
    vec![
        CatalogItem {
            category: Category::Appearance,
            name: "Theme — Color Swatches",
            draw: theme::draw,
        },
        CatalogItem {
            category: Category::Appearance,
            name: "Typography",
            draw: typography::draw,
        },
        CatalogItem {
            category: Category::Appearance,
            name: "Spacing",
            draw: spacing::draw,
        },
        CatalogItem {
            category: Category::Widget,
            name: "Widget — hint_text",
            draw: widgets::hint_text::draw,
        },
        CatalogItem {
            category: Category::Widget,
            name: "Widget — Divider (pane borders)",
            draw: widgets::divider::draw,
        },
        CatalogItem {
            category: Category::Widget,
            name: "Widget — Toast (card visual)",
            draw: widgets::toast::draw,
        },
        CatalogItem {
            category: Category::Widget,
            name: "Widget — Dialog (rename popup frame)",
            draw: widgets::dialog::draw,
        },
        CatalogItem {
            category: Category::Widget,
            name: "Widget — Multi-tier Tab Layout",
            draw: widgets::multi_tab_layout::draw,
        },
        CatalogItem {
            category: Category::Popup,
            name: "Popup — Markdown Open",
            draw: components::markdown_open::draw,
        },
        CatalogItem {
            category: Category::Popup,
            name: "Popup — Update (Tier 3)",
            draw: components::update::draw,
        },
        CatalogItem {
            category: Category::Component,
            name: "Component — Convert popup (props view)",
            draw: components::convert::draw,
        },
        CatalogItem {
            category: Category::Component,
            name: "Component — Port Scanner popup",
            draw: components::port_scanner::draw,
        },
        CatalogItem {
            category: Category::Popup,
            name: "Popup — Apply Preset (Workspace/Tab/Pane)",
            draw: components::apply_preset::draw,
        },
        CatalogItem {
            category: Category::Popup,
            name: "Popup — File Handler Picker",
            draw: components::file_handler_picker::draw,
        },
        CatalogItem {
            category: Category::Component,
            name: "Component — Approval popup",
            draw: components::approval::draw,
        },
        CatalogItem {
            category: Category::Popup,
            name: "Popup — Command Palette",
            draw: components::command_palette::draw,
        },
        CatalogItem {
            category: Category::Layout,
            name: "Layout — Sidebar (Full / Collapsed)",
            draw: components::sidebar::draw,
        },
        CatalogItem {
            category: Category::Layout,
            name: "Layout — Pane Tab Bar",
            draw: components::tab_bar::draw,
        },
        CatalogItem {
            category: Category::Popup,
            name: "Popup — Rename (workspace / tab)",
            draw: components::rename_popup::draw,
        },
        CatalogItem {
            category: Category::Component,
            name: "Component — Toast Stack (Tier 3)",
            draw: components::toast::draw,
        },
        CatalogItem {
            category: Category::Layout,
            name: "Overlay — Surface Highlights",
            draw: components::surface_highlights::draw,
        },
    ]
}
