//! 진입 화면(list view) — jsx `IeActionRow`. Export · Import 두 액션 행이 이 모양을 쓴다.
//!
//! 경계: 진입 화면에만 나오는 그리기다. 안내문 헬퍼는 detail 과 함께 쓰므로 `paint` 에 있다.

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{Button, ButtonVariant, ControlSize};

use crate::adapters::ui::icons;

use super::CARD_PAD_X;
use super::paint::glyph_at;

/// jsx `IeActionRow` — glyph · 제목(13 primary) + 설명(12 muted, measure-md) · trailing 버튼.
pub(super) fn action_row(
    ui: &mut egui::Ui,
    th: &Theme,
    glyph: icons::Icon,
    title: &str,
    desc: &str,
    button: &str,
    variant: ButtonVariant,
) -> bool {
    let mut clicked = false;
    egui::Frame::new()
        .fill(th.surface_raised().to_egui())
        .stroke(egui::Stroke::new(
            th.border_width.value(),
            th.border_default().to_egui(),
        ))
        .corner_radius(th.corner_radius.value())
        .inner_margin(tasty_ui_widgets::margin_sym(CARD_PAD_X, th.spacing_md))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = th.spacing_md.value();
                glyph_at(ui, glyph, th.icon_glyph_size_md, th.text_muted().to_egui());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    clicked = Button::new(button)
                        .variant(variant)
                        .size(ControlSize::Sm)
                        .show(ui, th)
                        .clicked();
                    ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                        ui.spacing_mut().item_spacing.y =
                            tasty_ui_widgets::tokens::STRUCT_GAP_2.value();
                        ui.label(
                            egui::RichText::new(title)
                                .size(th.font_size_body.value())
                                .color(th.text_primary()),
                        );
                        ui.scope(|ui| {
                            ui.set_max_width(th.measure_md.value());
                            ui.label(
                                egui::RichText::new(desc)
                                    .size(th.font_size_term_sm.value())
                                    .color(th.text_muted()),
                            );
                        });
                    });
                });
            });
        });
    clicked
}
