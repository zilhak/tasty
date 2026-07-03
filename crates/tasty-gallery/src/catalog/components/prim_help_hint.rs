//! `HelpHint · Tooltip` primitive specimen — 디자인(4) `gallery/components.jsx`
//! "HelpHint · Tooltip" Section 전사.
//!
//! 설정행 사용례(라벨 옆 `(?)`) · rest→hover 색 전환 · 4 placement 강제 open 버블.
//! 하단 `meta` 로 치수/토큰 노출.

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{HelpHint, TooltipPlacement};

use crate::catalog::spec::{StageVariant, TokenChip, cluster, meta, note, stage};

/// 4 placement 강제 open 데모의 예시 문장 — max-width 240 에서 줄바꿈을 보이도록 다문장.
const SAMPLE: &str =
    "Shows a short explanation on hover. Wraps at 240px so two or three sentences stay readable.";

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    stage(ui, theme, StageVariant::Column, |ui| {
        cluster(ui, theme, "in a settings row — the common case", |ui| {
            ui.spacing_mut().item_spacing.x = theme.spacing_xs.value();
            ui.label(
                egui::RichText::new("Blink cursor")
                    .size(theme.font_size_body.value())
                    .color(egui::Color32::from(theme.text_primary())),
            );
            HelpHint::new("When on, the block cursor blinks in focused terminals.")
                .placement(TooltipPlacement::Top)
                .show(ui, theme);
        });

        cluster(ui, theme, "rest · hover (hover the right glyph)", |ui| {
            ui.spacing_mut().item_spacing.x = theme.spacing_lg.value();
            // rest — 아무 상호작용 없이 muted 색.
            HelpHint::new(SAMPLE).show(ui, theme);
            // hover 시 secondary 색 + 150ms 후 버블 (라이브 확인용).
            HelpHint::new(SAMPLE)
                .placement(TooltipPlacement::Bottom)
                .show(ui, theme);
        });

        cluster(
            ui,
            theme,
            "placement — top · bottom · left · right",
            |ui| {
                // 강제 open 4종 — Area id 를 placement 별로 고유화(동시 표시 충돌 방지).
                ui.spacing_mut().item_spacing.x = theme.spacing_xl.value();
                for (place, key) in [
                    (TooltipPlacement::Top, "hh_top"),
                    (TooltipPlacement::Bottom, "hh_bottom"),
                    (TooltipPlacement::Left, "hh_left"),
                    (TooltipPlacement::Right, "hh_right"),
                ] {
                    HelpHint::new(SAMPLE)
                        .placement(place)
                        .open(true)
                        .id_source(key)
                        .show(ui, theme);
                }
            },
        );
    });

    note(
        ui,
        theme,
        "HelpHint = the (?) glyph + a Tooltip. The bubble is opaque (surface-raised, \
         border-strong, popover shadow) with no arrow — never egui's default on_hover_text.",
    );

    meta(
        ui,
        theme,
        &[
            ("glyph", "14px (?) · icon-size-sm"),
            ("gap to label", "4px · space-xs"),
            ("bubble", "surface-raised card · no arrow"),
            ("max-width", "240px · wraps"),
            ("text", "caption 11 · line-height 1.4"),
            ("delay", "150ms hover"),
        ],
        &[
            TokenChip::new(
                "text-muted",
                "rest glyph",
                egui::Color32::from(theme.text_muted()),
            ),
            TokenChip::new(
                "text-secondary",
                "hover glyph · bubble text",
                egui::Color32::from(theme.text_secondary()),
            ),
            TokenChip::new(
                "surface-raised",
                "bubble bg",
                egui::Color32::from(theme.surface_raised()),
            ),
            TokenChip::new(
                "border-strong",
                "bubble border",
                egui::Color32::from(theme.border_strong()),
            ),
        ],
    );
}
