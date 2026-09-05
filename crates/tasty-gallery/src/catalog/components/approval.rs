//! Agent approval — 디자인(4) Overlays `approval` Spec.
//!
//! 440px 모달. 헤더(agent dot + title + agent Tag, border-bottom) · 본문(설명 +
//! `pre` 명령 블록 + 권한 Tag) · footer(Deny / Allow once / Always).

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{BadgeVariant, Button, ButtonVariant, TagVariant, badge_dot, tag};

use crate::catalog::spec::{self, StageVariant, TokenChip};
use crate::catalog::widgets::dialog as kit;

const WIDTH: LogicalPx = LogicalPx(440.0);

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        kit::frame_card(ui, theme, WIDTH, kit::panel_fill(theme), |ui| {
            // 헤더 (padding 12x14).
            kit::region_sym(ui, theme.spacing_md, theme.spacing_md, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
                    badge_dot(ui, theme, BadgeVariant::Agent);
                    kit::title(ui, theme, "Approve agent action");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        tag(ui, theme, "agent", TagVariant::Agent, false);
                    });
                });
            });
            kit::hsep(ui, theme);

            // 본문 (padding 14, gap 10).
            kit::region_sym(ui, theme.spacing_md, theme.spacing_md, |ui| {
                ui.spacing_mut().item_spacing.y = theme.spacing_sm.value();
                kit::body(
                    ui,
                    theme,
                    "The agent ai-review wants to run a command in s_01HXK9:",
                );
                // pre cmd 블록 (#000 위 mono).
                egui::Frame::new()
                    .fill(theme.bg_app().to_egui())
                    .corner_radius(theme.corner_radius_sm.value())
                    .inner_margin(egui::Margin::same(theme.spacing_md.value() as i8))
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.label(
                            egui::RichText::new("git push --force origin main")
                                .monospace()
                                .size(theme.font_size_term_sm.value())
                                .color(theme.text_primary().to_egui()),
                        );
                    });
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
                    tag(ui, theme, "destructive", TagVariant::Danger, true);
                    tag(ui, theme, "fs:write", TagVariant::Default, false);
                    tag(ui, theme, "net", TagVariant::Default, false);
                });
            });
            kit::hsep(ui, theme);

            // footer (padding 10x14).
            kit::region_sym(ui, theme.spacing_md, theme.spacing_sm, |ui| {
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        Button::new("Always allow")
                            .variant(ButtonVariant::Agent)
                            .show(ui, theme);
                        Button::new("Allow once")
                            .variant(ButtonVariant::Secondary)
                            .show(ui, theme);
                        Button::new("Deny")
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
            ("frame", "440px · bg-panel"),
            ("header", "agent dot · title · agent Tag"),
            ("body", "prose · pre on bg-app · permission Tags"),
            ("footer", "Deny · Allow once · Always"),
        ],
        &[
            TokenChip::new(
                "accent-agent",
                "agent identity",
                theme.accent_agent().to_egui(),
            ),
            TokenChip::new("bg-app", "command block", theme.bg_app().to_egui()),
            TokenChip::new(
                "accent-warning",
                "risky grant",
                theme.accent_warning().to_egui(),
            ),
        ],
    );

    spec::note(
        ui,
        theme,
        "Mauve always means agent. The exact command and the permissions it grants \
         are shown verbatim before anything runs — Allow once is the safe default.",
    );
}
