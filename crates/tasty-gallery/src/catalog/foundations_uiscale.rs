//! Foundations UI-scale specimen — 디자인(4) "UI scale — sidebar zoom".
//!
//! Spec "One multiplier scales the sidebar; everything else stays fixed". 한 배율
//! (`ui-scale`)이 사이드바 루트의 zoom 으로만 소비된다. 3 stop(sm 0.8 / md 1.0 /
//! lg 1.2)을 같은 사이드바 행(StatusDot + 라벨 + Badge)에 적용해 비교한다.
//! 타이틀바·탭·페인·다이얼로그는 영향받지 않는다.
//!
//! `ui-scale` 배율 토큰은 아직 `Theme` 에 없어(연속 zoom 값) 디자인 stop 값을
//! 리터럴로 둔다 — 길이가 아니라 무차원 배율이다.

use tasty_type_appearance::theme::Theme;

use crate::catalog::spec::{cluster, meta, note, stage, StageVariant, TokenChip};

/// 디자인 `--tasty-ui-scale-{sm,md,lg}` stop 값.
const STOPS: [(&str, f32); 3] = [("sm · 0.8", 0.8), ("md · 1.0", 1.0), ("lg · 1.2", 1.2)];

#[inline]
fn ec(c: impl Into<egui::Color32>) -> egui::Color32 {
    c.into()
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    stage(ui, theme, StageVariant::Wrap, |ui| {
        for (label, scale) in STOPS {
            cluster(ui, theme, label, |ui| {
                scaled_sidebar_row(ui, theme, scale);
            });
        }
    });

    meta(
        ui,
        theme,
        &[
            ("stops", "0.8 / 1.0 / 1.2"),
            ("active value", "ui-scale"),
            ("consumed by", "sidebar root zoom only"),
            ("excluded", "title bar · tabs · panes · dialogs"),
            ("control", "Appearance › Display"),
        ],
        &[
            TokenChip::new("ui-scale-sm", "0.8", ec(theme.text_muted())),
            TokenChip::new("ui-scale-md", "1.0", ec(theme.text_secondary())),
            TokenChip::new("ui-scale-lg", "1.2", ec(theme.text_primary())),
            TokenChip::new("ui-scale", "active", ec(theme.accent_primary())),
        ],
    );
    note(
        ui,
        theme,
        "배율은 사이드바 루트의 zoom 으로만 적용된다 — 터미널 셀 크기·탭·다이얼로그는 그대로 고정.",
    );
}

/// 배율이 적용된 사이드바 행 — StatusDot(dot) + 라벨 + count Badge, 모든 치수 × scale.
fn scaled_sidebar_row(ui: &mut egui::Ui, theme: &Theme, scale: f32) {
    egui::Frame::new()
        .fill(ec(theme.bg_sidebar()))
        .stroke(egui::Stroke::new(theme.border_width.value(), ec(theme.separator)))
        .corner_radius(theme.corner_radius_sm.value())
        .inner_margin(egui::Margin {
            left: (theme.spacing_md.value() * scale) as i8,
            right: (theme.spacing_md.value() * scale) as i8,
            top: (theme.spacing_sm.value() * scale) as i8,
            bottom: (theme.spacing_sm.value() * scale) as i8,
        })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = theme.spacing_sm.value() * scale;
                // dot (agent).
                let d = theme.status_dot_size.value() * scale;
                let (r, _) = ui.allocate_exact_size(egui::vec2(d, d), egui::Sense::hover());
                ui.painter().circle_filled(r.center(), d * 0.5, ec(theme.accent_agent()));
                // 라벨.
                ui.label(
                    egui::RichText::new("agent · zsh")
                        .size(theme.font_size_body.value() * scale)
                        .color(ec(theme.text_secondary())),
                );
                // count badge.
                mini_badge(ui, theme, "2", scale);
            });
        });
}

/// 배율 적용 count pill — accent-danger fill + caption 텍스트.
fn mini_badge(ui: &mut egui::Ui, theme: &Theme, count: &str, scale: f32) {
    let font = theme.font_size_caption.value() * scale;
    let galley = ui.painter().layout_no_wrap(
        count.to_owned(),
        egui::FontId::proportional(font),
        ec(theme.text_on_accent()),
    );
    let pad = theme.spacing_xs.value() * scale;
    let h = galley.rect.height() + pad * 2.0;
    let w = galley.rect.width() + pad * 3.0;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w.max(h), h), egui::Sense::hover());
    ui.painter().rect_filled(rect, h * 0.5, ec(theme.accent_danger()));
    ui.painter().galley(
        rect.center() - galley.rect.size() * 0.5,
        galley,
        ec(theme.text_on_accent()),
    );
}
