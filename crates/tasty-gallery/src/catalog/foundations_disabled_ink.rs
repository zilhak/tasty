//! disabled 잉크 예제. 대비 목표 없이 순서와 테마 간 패리티만 지킨다.
//! C3 행은 port scanner 푸터를 공용 Button으로 그린다. C4 행(탭 스트립 스크롤 화살표)은
//! Layouts › Tab strips의 "Tab strip scroll arrows — disabled ink"에 있다.

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{Button, ButtonVariant};

use crate::catalog::spec::{StageVariant, TokenChip, dont, meta, stage};

#[inline]
fn ec(c: impl Into<egui::Color32>) -> egui::Color32 {
    c.into()
}

/// 팔레트 하나를 현재 UI 배율로 다시 만든다. 한 페이지에 Mocha와 Latte를 나란히 놓기 위해 쓴다.
fn palette(base: &Theme, zoom: f32) -> Theme {
    Theme::with_colors_and_zoom(base.to_colors(), base.is_light, zoom)
}

/// 행 머리의 작은 mono 설명.
fn caption(ui: &mut egui::Ui, th: &Theme, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .monospace()
            .size(th.font_size_micro.value())
            .color(ec(th.text_muted())),
    );
}

/// 한 팔레트 패널: C3 푸터 행과 잉크 단계 줄.
fn panel(ui: &mut egui::Ui, th: &Theme, name: &str, c3: &str) {
    egui::Frame::new()
        .fill(ec(th.bg_app()))
        .stroke(egui::Stroke::new(
            th.border_width.value(),
            ec(th.border_default()),
        ))
        .corner_radius(th.corner_radius.value())
        .inner_margin(egui::Margin::same(th.spacing_md.value() as i8))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = th.spacing_md.value();
            ui.label(
                egui::RichText::new(name)
                    .size(th.font_size_caption.value())
                    .color(ec(th.text_muted())),
            );
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = th.spacing_xs.value();
                caption(ui, th, c3);
                egui::Frame::new()
                    .fill(ec(th.bg_panel()))
                    .corner_radius(th.corner_radius.value())
                    .inner_margin(egui::Margin::same(th.spacing_md.value() as i8))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = th.spacing_sm.value();
                            Button::new("Close")
                                .variant(ButtonVariant::Secondary)
                                .show(ui, th);
                            Button::new("Copy address")
                                .variant(ButtonVariant::Ghost)
                                .enabled(false)
                                .show(ui, th);
                        });
                    });
            });
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = th.spacing_xs.value();
                for (label, color) in [
                    ("placeholder", th.text_placeholder()),
                    ("disabled", th.text_disabled()),
                    ("muted", th.text_muted()),
                    ("secondary", th.text_secondary()),
                    ("primary", th.text_primary()),
                ] {
                    egui::Frame::new()
                        .fill(ec(th.bg_panel()))
                        .corner_radius(th.corner_radius_sm.value())
                        .inner_margin(egui::Margin::symmetric(
                            th.spacing_sm.value() as i8,
                            th.spacing_xs.value() as i8,
                        ))
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new(label)
                                    .size(th.font_size_caption.value())
                                    .color(ec(color)),
                            );
                        });
                }
            });
        });
}

/// Foundations › Disabled ink — 대비 목표 없음, Latte 한 단계 위.
pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    let mocha = palette(&tasty_themes::mocha_fallback(), theme.ui_zoom);
    let latte = palette(&crate::host_shell::latte_theme(), theme.ui_zoom);
    stage(ui, theme, StageVariant::Column, |ui| {
        // 시안은 flex-wrap이다. C3 캡션이 길어 두 패널이 한 줄에 들어가지 않으므로 세로로 쌓인다.
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = theme.spacing_lg.value();
            panel(
                ui,
                &mocha,
                "Mocha",
                "C3 · port scanner footer (ink example; the footer stays Close + Copy address) — disabled 3.40:1 (was 3.40 (n700, unchanged))",
            );
            panel(
                ui,
                &latte,
                "Latte",
                "C3 · port scanner footer (ink example; the footer stays Close + Copy address) — disabled 2.56:1 (was 2.07 (n700))",
            );
        });
    });

    meta(
        ui,
        theme,
        &[
            ("A · target", "none — WCAG exempts disabled"),
            ("rule 1 · order", "placeholder < disabled < muted"),
            ("rule 2 · parity", "Latte remap n700 → n800"),
            (
                "B · ground",
                "n/a (no target) — report on the control's own ground",
            ),
            ("C · enabled arrow", "in scope → text-muted (3:1 non-text)"),
            ("one ink", "labels + glyphs, unchanged principle"),
            (
                "pixels",
                "Latte disabled everywhere · both themes' enabled arrow",
            ),
        ],
        &[
            TokenChip::new(
                "text-disabled",
                "Mocha n700 · Latte n800",
                ec(theme.text_disabled()),
            ),
            TokenChip::new(
                "tab-scroll-arrow-fg",
                "→ text-muted",
                ec(theme.tab_scroll_arrow_fg()),
            ),
            TokenChip::new(
                "tab-scroll-arrow-fg-disabled",
                "→ text-disabled",
                ec(theme.tab_scroll_arrow_fg_disabled()),
            ),
        ],
    );
    dont(
        ui,
        theme,
        "Don't lift disabled to 4.5:1. In Latte that lands on text-muted and disabled stops \
         reading as disabled. The hierarchy is the requirement; contrast is reported, not targeted.",
    );
}
