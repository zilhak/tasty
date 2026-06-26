//! `IconButton` primitive specimen — 디자인(4) `components/buttons/IconButton` 카드.
//!
//! ghost(테두리 없음)/solid/active × md/sm + disabled. 글리프는 `super::glyph`
//! (디자인 icons.json 미러). 하단 `meta` 로 치수/토큰 노출.

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{ControlSize, IconButton, IconButtonVariant};

use super::glyph;
use crate::catalog::spec::{StageVariant, TokenChip, cluster, meta, stage};

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    stage(ui, theme, StageVariant::Column, |ui| {
        cluster(ui, theme, "ghost · solid · active", |ui| {
            IconButton::new()
                .variant(IconButtonVariant::Ghost)
                .show(ui, theme, &|ui, rect, c| {
                    glyph::SPLIT.image(rect.height(), c).paint_at(ui, rect)
                });
            IconButton::new()
                .variant(IconButtonVariant::Solid)
                .show(ui, theme, &|ui, rect, c| {
                    glyph::SEARCH.image(rect.height(), c).paint_at(ui, rect)
                });
            IconButton::new()
                .active(true)
                .show(ui, theme, &|ui, rect, c| {
                    glyph::TERMINAL.image(rect.height(), c).paint_at(ui, rect)
                });
            IconButton::new()
                .size(ControlSize::Sm)
                .show(ui, theme, &|ui, rect, c| {
                    glyph::PLUS.image(rect.height(), c).paint_at(ui, rect)
                });
        });
    });

    meta(
        ui,
        theme,
        &[
            ("size", "28 square · sm 24"),
            ("radius", "4"),
            ("active", "accent-primary fill"),
            ("overlay", "hover 8%"),
        ],
        &[
            TokenChip::new(
                "overlay-hover",
                "hover 8%",
                egui::Color32::from(theme.overlay_hover()),
            ),
            TokenChip::new(
                "accent-primary",
                "active fill",
                egui::Color32::from(theme.accent_primary()),
            ),
            TokenChip::new(
                "text-muted",
                "glyph color",
                egui::Color32::from(theme.text_muted()),
            ),
        ],
    );
}
