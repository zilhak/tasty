//! `Spinner` primitive specimen — 디자인(4) `components/feedback/Spinner` 카드.
//!
//! 기본 spinner-size(16) 회전 arc + 저대비 track · 크기 램프 · accent 색 ·
//! reduced-motion 3-dot fallback. 하단 `meta` 로 치수/토큰 노출.

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{Button, ButtonVariant, Spinner};

use crate::catalog::spec::{StageVariant, TokenChip, cluster, meta, stage};

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    let base = theme.spinner_size.value();
    stage(ui, theme, StageVariant::Column, |ui| {
        cluster(ui, theme, "sizes — 12 · 16 · 20 · 24", |ui| {
            Spinner::new().size(12.0).show(ui, theme);
            Spinner::new().size(base).show(ui, theme);
            Spinner::new().size(20.0).show(ui, theme);
            Spinner::new().size(24.0).show(ui, theme);
        });
        cluster(ui, theme, "inline with text", |ui| {
            ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
            Spinner::new().size(14.0).show(ui, theme);
            ui.label(
                egui::RichText::new("Collecting…")
                    .size(theme.font_size_body.value())
                    .color(egui::Color32::from(theme.text_muted())),
            );
        });
        cluster(ui, theme, "in a button · detecting row", |ui| {
            // 디자인: <Button variant=secondary disabled leadingIcon={<Spinner 14/>}>Installing…</Button>.
            // Button.leading_icon 은 정적 글리프(IconPainter)만 받아 Spinner 를 임베드할 수
            // 없으므로, spinner 를 버튼 앞에 두어 근사한다(구조적 갭 — 요약 기록).
            Spinner::new().size(14.0).show(ui, theme);
            Button::new("Installing…")
                .variant(ButtonVariant::Secondary)
                .enabled(false)
                .show(ui, theme);
            ui.add_space(theme.spacing_md.value());
            ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
            Spinner::new().size(12.0).show(ui, theme);
            ui.label(
                egui::RichText::new("detecting shell…")
                    .size(theme.font_size_caption.value())
                    .monospace()
                    .color(egui::Color32::from(theme.text_muted())),
            );
        });
        cluster(ui, theme, "reduced motion — 3 static dots", |ui| {
            // 갤러리는 사용자 설정과 무관하게 **두 상태를 나란히** 보여야 하므로
            // 여기서만 override 를 쓴다(제품 화면은 `theme.reduced_motion` 을 따른다).
            ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
            Spinner::new()
                .reduced_motion(false)
                .size(base)
                .show(ui, theme);
            Spinner::new()
                .reduced_motion(true)
                .size(base)
                .show(ui, theme);
        });
    });

    meta(
        ui,
        theme,
        &[
            ("sizes", "12 / 16 / 20 / 24px"),
            ("stroke", "2px arc + faint track"),
            ("spin", "0.9s linear"),
            ("color", "currentColor"),
            ("reduced motion", "→ 3 static dots"),
        ],
        &[
            TokenChip::new(
                "text-muted",
                "default color",
                egui::Color32::from(theme.text_muted()),
            ),
            TokenChip::new(
                "spinner-size",
                "default 16",
                egui::Color32::from(theme.accent_primary()),
            ),
        ],
    );
}
