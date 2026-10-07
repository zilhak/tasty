//! 사이드바 Tools 버튼에 붙는 팝오버 예제. 배경을 어둡게 하지 않는다.
//! 항목은 본체 `tools_menu.rs` 의 내장 도구 순서와 번들 플러그인이 기여하는 도구(order_hint 순)다.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{MenuItemVariant, fit_menu_width, menu_item, menu_separator};

use crate::catalog::spec::{self, StageVariant, TokenChip};
use crate::catalog::widgets::dialog as kit;

/// 본체 내장 도구의 영어 라벨(lang/en.toml) — 본체 `BUILTIN_TOOLS` 순서.
const BUILTIN: &[&str] = &[
    "Command palette…",
    "Listening ports…",
    "Remote connections…",
    "Presets",
    "Tutorial…",
    "Task DAGs",
    "Open File…",
];
/// 번들 플러그인이 기여하는 도구 — clipboard-viewer(order_hint 100), git-viewer(130).
const PLUGIN: &[&str] = &["Clipboard Viewer", "Git"];

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    // 본체 `tools_menu.rs` 와 같은 계산: 가장 넓은 행을 min..max(테두리 포함)로 제한하고,
    // 셸 테두리는 바깥에 그리므로 테두리를 뺀 폭을 카드에 준다.
    let outer = fit_menu_width(
        ui.ctx(),
        theme,
        BUILTIN.iter().chain(PLUGIN).copied(),
        theme.tools_menu_min_width().value(),
        theme.tools_menu_max_width().value(),
    );
    let width = LogicalPx(outer - theme.border_width.value() * 2.0);
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        kit::frame_card_menu(ui, theme, width, |ui| {
            let ring = theme.popup_content_margin();
            kit::region_sym(ui, ring, ring, |ui| {
                for label in BUILTIN {
                    row(ui, theme, label);
                }
                menu_separator(ui, theme);
                for label in PLUGIN {
                    row(ui, theme, label);
                }
            });
        });
    });

    spec::meta(
        ui,
        theme,
        &[
            (
                "width",
                "fit content · min 160 tools-menu-min-width · max 240 tools-menu-max-width",
            ),
            ("overflow", "end ellipsis past max"),
            ("anchor", "above button, left-aligned"),
            ("rows", "28px MenuItem, flush, no icons"),
            (
                "label",
                "menu-item-fg → text-secondary · hover menu-item-fg-hover → text-primary",
            ),
            ("built-in", "7 — fixed order"),
            ("inner ring", "4 · popup-content-margin"),
            ("scrim", "none"),
            ("dismiss", "outside click · Esc"),
        ],
        &[
            TokenChip::new("menu-bg", "menu fill", theme.menu_bg().to_egui()),
            TokenChip::new(
                "menu-border",
                "edge → border-strong",
                theme.menu_border().to_egui(),
            ),
            TokenChip::without_color("popup-content-margin", "→ space-xs 4"),
            TokenChip::without_color("tools-menu-min-width", "→ size-160"),
            TokenChip::without_color("tools-menu-max-width", "→ size-240"),
            TokenChip::without_color("shadow-popover", "lift"),
        ],
    );

    spec::note(
        ui,
        theme,
        "Plugins extend the list below the separator only — built-in order is fixed. This is a popover, not a dialog: it never dims the app.",
    );
}

fn row(ui: &mut egui::Ui, theme: &Theme, label: &str) {
    menu_item(
        ui,
        theme,
        None,
        label,
        None,
        MenuItemVariant::Secondary,
        false,
        true,
    );
}
