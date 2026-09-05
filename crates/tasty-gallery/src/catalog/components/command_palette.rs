//! Command palette ⌘K — 디자인(4) Overlays `palette` Spec.
//!
//! 480px surface-raised 프레임, top-anchor. Input 헤더 + MenuItem 리스트(첫 active)
//! + mono 힌트 footer. 색·치수는 Theme 토큰, 프레임/필드는 공유 kit.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{MenuItemVariant, menu_item};

use crate::catalog::icons::{self, MockGlyph};
use crate::catalog::spec::{self, StageVariant, TokenChip};
use crate::catalog::widgets::dialog as kit;

const WIDTH: LogicalPx = LogicalPx(480.0);

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        kit::frame_card(ui, theme, WIDTH, kit::raised_fill(theme), |ui| {
            // 헤더 — 검색 Input (padding 10).
            kit::region_sym(
                ui,
                theme.spacing_md.value(),
                theme.spacing_sm.value(),
                |ui| {
                    kit::field(ui, theme, None, "Type to search commands…", true, false);
                },
            );
            kit::hsep(ui, theme);

            // 리스트 — MenuItem 행 (첫 active). padding 6.
            kit::region_sym(
                ui,
                theme.spacing_sm.value(),
                theme.spacing_sm.value(),
                |ui| {
                    row(ui, theme, icons::TERMINAL, "New Terminal", "Ctrl+T", true);
                    row(
                        ui,
                        theme,
                        icons::SPLIT,
                        "Split Pane Vertical",
                        "Ctrl+D",
                        false,
                    );
                    row(
                        ui,
                        theme,
                        icons::PORT,
                        "Toggle Theme (Mocha / Latte)",
                        "",
                        false,
                    );
                    row(ui, theme, icons::SETTINGS, "Settings", "Ctrl+,", false);
                },
            );
            kit::hsep(ui, theme);

            // footer — mono 힌트 (padding 8x12).
            kit::region_sym(
                ui,
                theme.spacing_md.value(),
                theme.spacing_sm.value(),
                |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = theme.spacing_lg.value();
                        kit::caption(ui, theme, "↑↓ navigate", true);
                        kit::caption(ui, theme, "↵ run", true);
                        kit::caption(ui, theme, "esc close", true);
                    });
                },
            );
        });
    });

    spec::meta(
        ui,
        theme,
        &[
            ("frame", "480px · surface-raised"),
            ("anchor", "top · overlay-top-offset 88"),
            ("header", "Input · padding 10 · border-bottom"),
            ("list", "MenuItem · padding 6 · first active"),
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
        ],
    );

    spec::note(
        ui,
        theme,
        "Keyboard-first: opens under the title bar, first result pre-selected, \
         arrows move and Enter runs. The scrim dismisses on click or Esc.",
    );
}

fn row(ui: &mut egui::Ui, theme: &Theme, glyph: MockGlyph, label: &str, sc: &str, active: bool) {
    let shortcut = if sc.is_empty() { None } else { Some(sc) };
    menu_item(
        ui,
        theme,
        Some(&|ui, rect, c| glyph.image(rect.height(), c).paint_at(ui, rect)),
        label,
        shortcut,
        MenuItemVariant::Normal,
        active,
        true,
    );
}
