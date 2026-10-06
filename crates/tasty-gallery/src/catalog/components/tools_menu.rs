//! 사이드바 Tools 버튼에 붙는 팝오버 예제. 배경을 어둡게 하지 않는다.
//! 항목은 본체 `tools_menu.rs` 의 내장 도구 순서와 번들 플러그인이 기여하는 도구(order_hint 순)다.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{MenuItemVariant, menu_item, menu_separator};

use crate::catalog::spec::{self, StageVariant, TokenChip};
use crate::catalog::widgets::dialog as kit;

const WIDTH: LogicalPx = LogicalPx(160.0);

/// 본체 내장 도구의 영어 라벨(lang/en.toml) — 본체 `BUILTIN_TOOLS` 순서.
const BUILTIN: &[&str] = &[
    "Command palette…",
    "Listening ports...",
    "Remote connections…",
    "Presets",
    "Tutorial…",
    "Task DAGs",
    "Open File…",
];
/// 번들 플러그인이 기여하는 도구 — clipboard-viewer(order_hint 100), git-viewer(130).
const PLUGIN: &[&str] = &["Clipboard Viewer", "Git"];

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        kit::frame_card_popover(ui, theme, WIDTH, kit::raised_fill(theme), |ui| {
            kit::region_sym(ui, theme.spacing_sm, theme.spacing_sm, |ui| {
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
            ("width", "160px"),
            ("anchor", "above button, left-aligned"),
            ("rows", "28px MenuItem, no icons"),
            ("scrim", "none"),
            ("dismiss", "outside click · Esc"),
        ],
        &[
            TokenChip::new(
                "surface-raised",
                "menu fill",
                theme.surface_raised().to_egui(),
            ),
            TokenChip::new("border-strong", "edge", theme.border_strong().to_egui()),
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
        MenuItemVariant::Normal,
        false,
        true,
    );
}
