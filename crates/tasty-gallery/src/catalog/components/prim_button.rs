//! `Button` primitive specimen — 디자인(4) `components/buttons/Button` 카드.
//!
//! 본체 팝업과 **동일한** `tasty_ui_widgets::Button` 을 호출한다(mirror 아님 —
//! demo=main). variant(primary/secondary/ghost/danger/agent) × size(sm/md/lg) ×
//! icon × state 를 `cluster` 로 묶고, 하단 `meta` 로 치수/토큰을 노출한다.

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{Button, ButtonVariant, ControlSize};

use super::glyph;
use crate::catalog::spec::{StageVariant, TokenChip, cluster, meta, stage};

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    stage(ui, theme, StageVariant::Column, |ui| {
        cluster(ui, theme, "variants — hover & click them", |ui| {
            Button::new("Save")
                .variant(ButtonVariant::Primary)
                .show(ui, theme);
            Button::new("Open folder")
                .variant(ButtonVariant::Secondary)
                .show(ui, theme);
            Button::new("Cancel")
                .variant(ButtonVariant::Ghost)
                .show(ui, theme);
            Button::new("Force detach")
                .variant(ButtonVariant::Danger)
                .show(ui, theme);
            Button::new("Run agent task")
                .variant(ButtonVariant::Agent)
                .show(ui, theme);
        });
        cluster(ui, theme, "sizes — sm 24 · md 28 · lg 32", |ui| {
            Button::new("Small")
                .variant(ButtonVariant::Secondary)
                .size(ControlSize::Sm)
                .show(ui, theme);
            Button::new("Medium")
                .variant(ButtonVariant::Secondary)
                .size(ControlSize::Md)
                .show(ui, theme);
            Button::new("Large")
                .variant(ButtonVariant::Secondary)
                .size(ControlSize::Lg)
                .show(ui, theme);
        });
        cluster(ui, theme, "with icons · disabled", |ui| {
            Button::new("New tab")
                .variant(ButtonVariant::Secondary)
                .leading_icon(&|ui, rect, c| glyph::PLUS.image(rect.height(), c).paint_at(ui, rect))
                .show(ui, theme);
            Button::new("Search")
                .variant(ButtonVariant::Primary)
                .trailing_icon(&|ui, rect, c| {
                    glyph::SEARCH.image(rect.height(), c).paint_at(ui, rect)
                })
                .show(ui, theme);
            Button::new("Disabled")
                .variant(ButtonVariant::Secondary)
                .enabled(false)
                .show(ui, theme);
        });
    });

    meta(
        ui,
        theme,
        &[
            ("height", "28 · sm 24 · lg 32"),
            ("padding", "0 space-md"),
            ("radius", "4"),
            ("overlay", "hover 8% · active 12%"),
            ("focus", "2px ring"),
        ],
        &[
            TokenChip::new(
                "accent-primary",
                "primary fill",
                egui::Color32::from(theme.accent_primary()),
            ),
            TokenChip::new(
                "accent-danger",
                "danger fill",
                egui::Color32::from(theme.accent_danger()),
            ),
            TokenChip::new(
                "accent-agent",
                "agent fill",
                egui::Color32::from(theme.accent_agent()),
            ),
            TokenChip::new(
                "overlay-hover",
                "hover 8%",
                egui::Color32::from(theme.overlay_hover()),
            ),
            TokenChip::new(
                "text-on-accent",
                "label on fill",
                egui::Color32::from(theme.text_on_accent()),
            ),
        ],
    );
}
