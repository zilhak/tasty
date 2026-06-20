//! `Button` — 텍스트 버튼 (디자인 `components/core/Button`).
//!
//! 디자인 계약:
//! - variant: `primary`/`agent`/`danger`(채움) · `secondary`(외곽선) · `ghost`(맨몸).
//! - size: sm(24) / md(28, 기본) / lg(32). radius `corner_radius`, gap `space-sm`.
//! - primary/agent/danger: 각 accent 채움 + `text-on-accent`.
//! - secondary: `surface-raised` + `border-default`(hover `border-strong`) + `text-primary`.
//! - ghost: 투명 + `text-secondary`(hover `text-primary`).
//! - hover/active overlay 틴트(`::after`) = `overlay-hover`/`overlay-active`.
//! - disabled: opacity 0.45.
//!
//! Motion(디자인 .prompt.md): rest/hover/active/disabled 채움이 canonical, hover
//! 틴트 fade 는 장식 → 즉시모드 스냅. (focus-ring 은 기능이나 텍스트 버튼은 해당 없음.)
//!
//! egui 한계: 폰트 weight(medium/semibold)는 별도 bold family 없이는 재현 불가 →
//! semibold variant 는 `text-on-accent`/`text-primary` 색으로만 강조(기존 tasty 관례).
//! 시각·sizing 통제를 위해 egui `Button` 대신 직접 painter 로 그린다.

use tasty_type_appearance::theme::Theme;

use crate::control::ControlSize;

/// 디자인 Button variant.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ButtonVariant {
    Primary,
    Secondary,
    Ghost,
    Danger,
    Agent,
}

/// Button 빌더.
pub struct Button<'a> {
    label: &'a str,
    variant: ButtonVariant,
    size: ControlSize,
    enabled: bool,
    /// 컨테이너 폭을 채운다(디자인 `block`).
    block: bool,
}

impl<'a> Button<'a> {
    pub fn new(label: &'a str) -> Self {
        Self {
            label,
            variant: ButtonVariant::Primary,
            size: ControlSize::Md,
            enabled: true,
            block: false,
        }
    }

    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn size(mut self, size: ControlSize) -> Self {
        self.size = size;
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn block(mut self, block: bool) -> Self {
        self.block = block;
        self
    }

    pub fn show(self, ui: &mut egui::Ui, theme: &Theme) -> egui::Response {
        let height = self.size.height(theme);
        let pad_x = self.size.pad_x(theme);
        let radius = theme.corner_radius.value();
        let bw = theme.border_width.value();

        // 텍스트 galley (UI proportional). 색은 PLACEHOLDER 로 두고 그릴 때 주입.
        let font_id = egui::FontId::proportional(self.size.font_size(theme));
        let galley =
            ui.painter()
                .layout_no_wrap(self.label.to_owned(), font_id, egui::Color32::PLACEHOLDER);

        let content_w = galley.rect.size().x + 2.0 * pad_x;
        let desired_w = if self.block {
            ui.available_width().max(content_w)
        } else {
            content_w
        };
        let sense = if self.enabled {
            egui::Sense::click()
        } else {
            egui::Sense::hover()
        };
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(desired_w, height), sense);

        let op = |c: egui::Color32| {
            if self.enabled {
                c
            } else {
                c.gamma_multiply(0.45)
            }
        };

        // variant 별 base fill / border / fg.
        let (fill, border, fg) = match self.variant {
            ButtonVariant::Primary => (
                Some(theme.accent_primary().to_egui()),
                None,
                theme.text_on_accent().to_egui(),
            ),
            ButtonVariant::Agent => (
                Some(theme.accent_agent().to_egui()),
                None,
                theme.text_on_accent().to_egui(),
            ),
            ButtonVariant::Danger => (
                Some(theme.accent_danger().to_egui()),
                None,
                theme.text_on_accent().to_egui(),
            ),
            ButtonVariant::Secondary => {
                let b = if self.enabled && resp.hovered() {
                    theme.border_strong()
                } else {
                    theme.border_default()
                };
                (
                    Some(theme.surface_raised().to_egui()),
                    Some(b.to_egui()),
                    theme.text_primary().to_egui(),
                )
            }
            ButtonVariant::Ghost => {
                let f = if self.enabled && resp.hovered() {
                    theme.text_primary()
                } else {
                    theme.text_secondary()
                };
                (None, None, f.to_egui())
            }
        };

        if let Some(f) = fill {
            ui.painter().rect_filled(rect, radius, op(f));
        }
        if let Some(b) = border {
            ui.painter().rect_stroke(
                rect,
                radius,
                egui::Stroke::new(bw, op(b)),
                egui::StrokeKind::Inside,
            );
        }
        // hover/active 오버레이 틴트(::after) — 장식, 스냅.
        if self.enabled {
            if resp.is_pointer_button_down_on() {
                ui.painter()
                    .rect_filled(rect, radius, theme.overlay_active().to_egui_premultiplied());
            } else if resp.hovered() {
                ui.painter()
                    .rect_filled(rect, radius, theme.overlay_hover().to_egui_premultiplied());
            }
        }

        // 텍스트 — galley(PLACEHOLDER) 를 fg 색으로 중앙 배치.
        let text_pos = rect.center() - galley.rect.size() * 0.5;
        ui.painter().galley(text_pos, galley, op(fg));

        resp
    }
}
