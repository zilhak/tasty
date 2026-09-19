//! Command palette ⌘K — 디자인(4) Overlays `palette` Spec.
//!
//! 480px surface-raised 프레임, top-anchor. Input 헤더 + MenuItem 리스트(첫 active)
//! + mono 힌트 footer. 색·치수는 Theme 토큰, 프레임/필드는 공유 kit.

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
            // 헤더 — 검색 Input (padding 10).
            kit::region_sym(ui, theme.spacing_md, theme.spacing_sm, |ui| {
                kit::field(ui, theme, None, "Type to search commands…", true, false);
            });
            kit::hsep(ui, theme);

            // 리스트 — MenuItem 행 (첫 active). padding 6.
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
                // 단축키가 없는 명령 — 키캡 자리가 비는 행도 한 장에 함께 보인다.
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

            // footer — mono 힌트 (padding 8x12).
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

/// 팔레트 명령 행 — MenuItem 에 **키별 키캡**을 단다.
///
/// 단일 문자열(`"Ctrl+T"`)이 아니라 나뉜 토큰을 넘긴다. 한동안 이 미러는 문자열을
/// menu_item 의 텍스트 단축키 자리에 넣어, 본체가 키캡으로 그리는 것을 한 덩이 mono
/// 텍스트로 보이고 있었다 — specimen 이 보여야 할 컴포넌트가 화면에 없었다는 뜻이다.
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
