//! 카탈로그 항목 등록.
//!
//! 한 항목은 `(name, draw)` 의 페어. `draw` 는 선택 시 우측 디테일 패널에
//! 호출되는 함수로, `Theme` 을 받아 egui 위젯을 그린다.

pub mod spacing;
pub mod theme;
pub mod typography;
pub mod widgets;

use tasty_type_appearance::theme::Theme;

/// 좌측 사이드바에 표시되는 카탈로그 한 항목.
#[derive(Clone, Copy)]
pub struct CatalogItem {
    pub name: &'static str,
    pub draw: fn(&mut egui::Ui, &Theme),
}

/// 모든 카탈로그 항목. 좌측 트리는 이 목록을 순회한다.
pub fn all() -> Vec<CatalogItem> {
    vec![
        CatalogItem {
            name: "Theme — Color Swatches",
            draw: theme::draw,
        },
        CatalogItem {
            name: "Typography",
            draw: typography::draw,
        },
        CatalogItem {
            name: "Spacing",
            draw: spacing::draw,
        },
        CatalogItem {
            name: "Widget — hint_text",
            draw: widgets::hint_text::draw,
        },
        CatalogItem {
            name: "Widget — Divider (pane borders)",
            draw: widgets::divider::draw,
        },
        CatalogItem {
            name: "Widget — Toast (card visual)",
            draw: widgets::toast::draw,
        },
        CatalogItem {
            name: "Widget — Dialog (rename popup frame)",
            draw: widgets::dialog::draw,
        },
    ]
}
