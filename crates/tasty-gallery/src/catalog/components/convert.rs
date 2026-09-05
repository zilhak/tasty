//! Convert surface — 디자인(4) Overlays `convert` Spec.
//!
//! 400px 모달. title + From(readonly Tag) → swap icon → To(Select) + hint +
//! Cancel/Convert. surface 타입을 그 자리에서 바꾼다.

use std::cell::RefCell;

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{Button, ButtonVariant, TagVariant, select, tag};

use crate::catalog::icons;
use crate::catalog::spec::{self, StageVariant, TokenChip};
use crate::catalog::widgets::dialog as kit;

const WIDTH: LogicalPx = LogicalPx(400.0);

thread_local! {
    static TO_SEL: RefCell<usize> = const { RefCell::new(1) };
}

const TYPES: &[&str] = &["markdown", "editor", "log viewer"];

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        kit::frame_card(ui, theme, WIDTH, kit::panel_fill(theme), |ui| {
            kit::region_sym(ui, theme.spacing_md, theme.spacing_md, |ui| {
                ui.spacing_mut().item_spacing.y = theme.spacing_md.value();
                kit::title(ui, theme, "Convert surface");
                // From → To 행.
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = theme.spacing_md.value();
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = theme.spacing_xs.value();
                        kit::caption(ui, theme, "From", false);
                        tag(ui, theme, "terminal", TagVariant::Default, false);
                    });
                    kit::icon(
                        ui,
                        icons::CHEVRON_RIGHT,
                        theme.icon_glyph_size_md,
                        theme.text_muted().to_egui(),
                    );
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = theme.spacing_xs.value();
                        kit::caption(ui, theme, "To", false);
                        TO_SEL.with(|s| {
                            let mut sel = s.borrow_mut();
                            select(
                                ui,
                                theme,
                                "convert_to",
                                &mut sel,
                                TYPES,
                                theme.field_width_md.value(),
                                true,
                            );
                        });
                    });
                });
                kit::caption(
                    ui,
                    theme,
                    "The running process keeps its scrollback; only the surface renderer changes.",
                    false,
                );
                // footer 버튼.
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        Button::new("Convert")
                            .variant(ButtonVariant::Primary)
                            .show(ui, theme);
                        Button::new("Cancel")
                            .variant(ButtonVariant::Ghost)
                            .show(ui, theme);
                    });
                });
            });
        });
    });

    spec::meta(
        ui,
        theme,
        &[
            ("frame", "400px · bg-panel"),
            ("from", "readonly Tag (current type)"),
            ("to", "Select · field-width 160"),
            ("hint", "11px caption"),
            ("footer", "Cancel · Convert"),
        ],
        &[
            TokenChip::new("bg-panel", "frame", theme.bg_panel().to_egui()),
            TokenChip::new(
                "surface-raised",
                "Select trigger",
                theme.surface_raised().to_egui(),
            ),
            TokenChip::new("text-muted", "hint", theme.text_muted().to_egui()),
        ],
    );
}
