//! 명령 팔레트의 검색 필드, 명령 행, 키보드 안내 예제.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{MenuItemVariant, menu_item_kbd};

use crate::catalog::icons::{self, MockGlyph};
use crate::catalog::spec::{self, StageVariant, TokenChip};
use crate::catalog::widgets::dialog as kit;

const WIDTH: LogicalPx = LogicalPx(480.0);

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        kit::frame_card(ui, theme, WIDTH, kit::raised_fill(theme), |ui| {
            kit::region_sym(ui, theme.spacing_md, theme.spacing_sm, |ui| {
                kit::field(ui, theme, None, "Type to search commands…", true, false);
            });
            kit::hsep(ui, theme);

            kit::region_sym(ui, theme.spacing_sm, theme.spacing_sm, |ui| {
                row(
                    ui,
                    theme,
                    icons::TERMINAL,
                    "New Terminal",
                    &["Ctrl", "T"],
                    true,
                );
                row(
                    ui,
                    theme,
                    icons::SPLIT,
                    "Split Pane Vertical",
                    &["Ctrl", "D"],
                    false,
                );
                row(
                    ui,
                    theme,
                    icons::PORT,
                    "Toggle Theme (Mocha / Latte)",
                    &[],
                    false,
                );
                row(
                    ui,
                    theme,
                    icons::SETTINGS,
                    "Settings",
                    &["Ctrl", ","],
                    false,
                );
            });
            kit::hsep(ui, theme);

            kit::region_sym(ui, theme.spacing_md, theme.spacing_sm, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = theme.spacing_lg.value();
                    kit::caption(ui, theme, "↑↓ navigate", true);
                    kit::caption(ui, theme, "↵ run", true);
                    kit::caption(ui, theme, "esc close", true);
                });
            });
        });
    });

    spec::meta(
        ui,
        theme,
        &[
            ("frame", "480px · surface-raised"),
            ("anchor", "top · overlay-top-offset 88"),
            ("header", "Input · padding 10 · border-bottom"),
            ("list", "MenuItem · padding 6 · first active · Kbd keycaps"),
            ("footer", "mono hints · padding 8×12 · gap 14"),
        ],
        &[
            TokenChip::new("surface-raised", "frame", theme.surface_raised().to_egui()),
            TokenChip::new(
                "surface-active",
                "active row",
                theme.surface_active().to_egui(),
            ),
            TokenChip::new("accent-primary", "match", theme.accent_primary().to_egui()),
            TokenChip::new("kbd-bg", "keycap fill", theme.kbd_bg().to_egui()),
        ],
    );

    spec::note(
        ui,
        theme,
        "Keyboard-first: opens under the title bar, first result pre-selected, \
         arrows move and Enter runs. The scrim dismisses on click or Esc.",
    );
}

/// 각 키를 별도 키캡으로 그리도록 단축키를 나누어 전달한다.
fn row(
    ui: &mut egui::Ui,
    theme: &Theme,
    glyph: MockGlyph,
    label: &str,
    keys: &[&str],
    active: bool,
) {
    menu_item_kbd(
        ui,
        theme,
        Some(&|ui, rect, c| glyph.image(rect.height(), c).paint_at(ui, rect)),
        label,
        keys,
        MenuItemVariant::Normal,
        active,
        true,
    );
}
