//! Tools menu — 디자인(4) Overlays `tools` Spec.
//!
//! 160px 팝오버 메뉴. 사이드바 하단(Tools 버튼)에 anchored, **scrim 없음**.
//! builtin 4 + separator + plugin 2. 색·치수는 Theme 토큰.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{MenuItemVariant, menu_item, menu_separator};

use crate::catalog::icons::{self, MockGlyph};
use crate::catalog::spec::{self, StageVariant, TokenChip};
use crate::catalog::widgets::dialog as kit;

const WIDTH: LogicalPx = LogicalPx(160.0);

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        kit::frame_card(ui, theme, WIDTH, kit::raised_fill(theme), |ui| {
            kit::region_sym(
                ui,
                theme.spacing_sm.value(),
                theme.spacing_sm.value(),
                |ui| {
                    row(ui, theme, icons::PORT, "Command palette…", false);
                    row(ui, theme, icons::REMOTE, "Listening ports...", false);
                    row(ui, theme, icons::SETTINGS, "Remote connections…", false);
                    row(ui, theme, icons::PLUG, "Presets", false);
                    menu_separator(ui, theme);
                    row(ui, theme, icons::CLIPBOARD, "Clipboard Viewer", false);
                    row(ui, theme, icons::SEARCH, "Git", false);
                },
            );
        });
    });

    spec::meta(
        ui,
        theme,
        &[
            ("frame", "160px · surface-raised"),
            ("anchor", "sidebar Tools button · no scrim"),
            ("shadow", "popover — 0 8px 28px /.4"),
            ("groups", "builtin · separator · plugins"),
        ],
        &[
            TokenChip::new("surface-raised", "frame", theme.surface_raised().to_egui()),
            TokenChip::new(
                "overlay-hover",
                "row hover",
                theme.overlay_hover().to_egui_premultiplied(),
            ),
            TokenChip::new("separator", "group divide", theme.separator.to_egui()),
        ],
    );

    spec::note(
        ui,
        theme,
        "Unlike a modal this is a lightweight popover — no scrim, anchored to its \
         trigger, and it closes when focus leaves. Plugins append below a separator.",
    );
}

fn row(ui: &mut egui::Ui, theme: &Theme, glyph: MockGlyph, label: &str, active: bool) {
    menu_item(
        ui,
        theme,
        Some(&|ui, rect, c| glyph.image(rect.height(), c).paint_at(ui, rect)),
        label,
        None,
        MenuItemVariant::Normal,
        active,
        true,
    );
}
