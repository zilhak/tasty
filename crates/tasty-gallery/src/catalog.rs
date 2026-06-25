//! 카탈로그 항목 등록.
//!
//! 한 항목은 `(name, draw)` 의 페어. `draw` 는 선택 시 우측 디테일 패널에
//! 호출되는 함수로, `Theme` 을 받아 egui 위젯을 그린다.
//!
//! 1 차 분류는 웹 디자인 시스템 gallery 와 동일한 4 분류:
//! Foundations / Components / Overlays / Layouts.
//! (이전 5 분류 Appearance/Widget/Popup/Component/Layout 에서 Widget+Component 를
//!  Components 로 통합하고, Popup 을 Overlays, Appearance 를 Foundations 로 재편.)

pub mod components;
pub mod icons;
pub mod popup_frame;
pub mod spacing;
pub mod specimen;
pub mod theme;
pub mod toast_card;
pub mod typography;
pub mod widgets;

use tasty_type_appearance::theme::Theme;

/// 카탈로그 1차 분류. 상단 탭 + 좌측 사이드바 필터링에 사용.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    /// 토큰·기초 (색/타입/간격).
    Foundations,
    /// 위젯·컴포넌트 (단일 UI 요소).
    Components,
    /// canonical 글리프 세트.
    Icons,
    /// 모달·팝업 레이어.
    Overlays,
    /// 구조 셸 (사이드바/탭바/분할/하이라이트).
    Layouts,
}

impl Category {
    pub fn label(self) -> &'static str {
        match self {
            Category::Foundations => "Foundations",
            Category::Components => "Components",
            Category::Icons => "Icons",
            Category::Overlays => "Overlays",
            Category::Layouts => "Layouts",
        }
    }

    pub fn all() -> &'static [Category] {
        &[
            Category::Foundations,
            Category::Components,
            Category::Icons,
            Category::Overlays,
            Category::Layouts,
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
        // ── Foundations ──
        CatalogItem {
            category: Category::Foundations,
            name: "Color Swatches",
            draw: theme::draw,
        },
        CatalogItem {
            category: Category::Foundations,
            name: "Typography",
            draw: typography::draw,
        },
        CatalogItem {
            category: Category::Foundations,
            name: "Spacing",
            draw: spacing::draw,
        },
        // ── Components ── (primitive 먼저 — 디자인 gallery components.html 순서)
        CatalogItem {
            category: Category::Components,
            name: "Button",
            draw: components::prim_button::draw,
        },
        CatalogItem {
            category: Category::Components,
            name: "IconButton",
            draw: components::prim_icon_button::draw,
        },
        CatalogItem {
            category: Category::Components,
            name: "Input",
            draw: components::prim_input::draw,
        },
        CatalogItem {
            category: Category::Components,
            name: "Badge · Tag · Kbd",
            draw: components::prim_chips::draw,
        },
        CatalogItem {
            category: Category::Components,
            name: "Select · Checkbox · Switch",
            draw: components::prim_forms::draw,
        },
        CatalogItem {
            category: Category::Components,
            name: "StatusDot",
            draw: components::prim_status_dot::draw,
        },
        CatalogItem {
            category: Category::Components,
            name: "Spinner",
            draw: components::prim_spinner::draw,
        },
        CatalogItem {
            category: Category::Components,
            name: "MenuItem · TreeRow",
            draw: components::prim_nav::draw,
        },
        CatalogItem {
            category: Category::Components,
            name: "Hint text",
            draw: widgets::hint_text::draw,
        },
        CatalogItem {
            category: Category::Components,
            name: "Toast (card visual)",
            draw: widgets::toast::draw,
        },
        // ── Icons ── (canonical 글리프 세트 — 디자인 gallery/icons.jsx)
        CatalogItem {
            category: Category::Icons,
            name: "Icon Set (canonical glyphs)",
            draw: icons::draw,
        },
        // ── Overlays ── (통팝업/컴포지션 — 디자인 gallery 구조: primitive 는 Components)
        CatalogItem {
            category: Category::Overlays,
            name: "Dialog (rename popup frame)",
            draw: widgets::dialog::draw,
        },
        CatalogItem {
            category: Category::Overlays,
            name: "Convert popup (props view)",
            draw: components::convert::draw,
        },
        CatalogItem {
            category: Category::Overlays,
            name: "Port Scanner popup",
            draw: components::port_scanner::draw,
        },
        CatalogItem {
            category: Category::Overlays,
            name: "Approval popup",
            draw: components::approval::draw,
        },
        CatalogItem {
            category: Category::Overlays,
            name: "Toast Stack (Tier 3)",
            draw: components::toast::draw,
        },
        CatalogItem {
            category: Category::Overlays,
            name: "Markdown Open",
            draw: components::markdown_open::draw,
        },
        CatalogItem {
            category: Category::Overlays,
            name: "Update (Tier 3)",
            draw: components::update::draw,
        },
        CatalogItem {
            category: Category::Overlays,
            name: "Apply Preset (Workspace/Tab/Pane)",
            draw: components::apply_preset::draw,
        },
        CatalogItem {
            category: Category::Overlays,
            name: "File Handler Picker",
            draw: components::file_handler_picker::draw,
        },
        CatalogItem {
            category: Category::Overlays,
            name: "Command Palette",
            draw: components::command_palette::draw,
        },
        CatalogItem {
            category: Category::Overlays,
            name: "Rename (workspace / tab)",
            draw: components::rename_popup::draw,
        },
        CatalogItem {
            category: Category::Overlays,
            name: "Search Bar",
            draw: components::search_bar::draw,
        },
        CatalogItem {
            category: Category::Overlays,
            name: "Tools Menu",
            draw: components::tools_menu::draw,
        },
        // ── Layouts ──
        CatalogItem {
            category: Category::Layouts,
            name: "Sidebar (Full / Collapsed)",
            draw: components::sidebar::draw,
        },
        CatalogItem {
            category: Category::Layouts,
            name: "Pane Tab Bar",
            draw: components::tab_bar::draw,
        },
        CatalogItem {
            category: Category::Layouts,
            name: "Divider (pane borders)",
            draw: widgets::divider::draw,
        },
        CatalogItem {
            category: Category::Layouts,
            name: "Surface Highlights",
            draw: components::surface_highlights::draw,
        },
        CatalogItem {
            category: Category::Layouts,
            name: "Multi-tier Tab Layout",
            draw: widgets::multi_tab_layout::draw,
        },
        CatalogItem {
            category: Category::Layouts,
            name: "1 depth (Plugins idiom)",
            draw: widgets::layout_1depth::draw,
        },
        CatalogItem {
            category: Category::Layouts,
            name: "2 depth (Settings idiom)",
            draw: widgets::layout_2depth::draw,
        },
    ]
}
