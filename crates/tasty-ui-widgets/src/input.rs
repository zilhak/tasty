//! `Input` — 단일 행 텍스트 필드 (디자인 `components/forms/Input`).
//!
//! 디자인 계약:
//! - height `control-height`(28), padding `0 space-md`, gap `space-sm`.
//! - bg `surface-raised`, border 1px `border-default`, radius `corner_radius`.
//! - focus: border `border-focus` + 1px ring(box-shadow 대체).
//! - invalid: border `accent-danger`. disabled: opacity 0.5.
//! - leading `icon`(15px, text-muted) / trailing `addon`(mono 11, text-muted).
//! - `mono`: 입력 폰트 monospace + caption. placeholder: `text-placeholder`.
//!
//! Motion(디자인 .prompt.md): 유일한 애니메이션은 focus 시 border/ring easing
//! (장식, 스냅 OK). focus-ring **가시성**과 `invalid` border 는 기능 → **즉시**
//! (여기선 fade 없이 즉시 그린다).

use tasty_type_appearance::theme::Theme;

use crate::icon_button::IconPainter;
use crate::tokens;

/// Input 빌더.
pub struct Input<'a> {
    placeholder: &'a str,
    mono: bool,
    invalid: bool,
    enabled: bool,
    /// 고정 폭. `None` 이면 가용 폭을 채운다(디자인 `block`).
    width: Option<f32>,
    icon: Option<IconPainter<'a>>,
    addon: Option<&'a str>,
}

impl Default for Input<'_> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> Input<'a> {
    pub fn new() -> Self {
        Self {
            placeholder: "",
            mono: false,
            invalid: false,
            enabled: true,
            width: None,
            icon: None,
            addon: None,
        }
    }

    pub fn placeholder(mut self, placeholder: &'a str) -> Self {
        self.placeholder = placeholder;
        self
    }

    pub fn mono(mut self, mono: bool) -> Self {
        self.mono = mono;
        self
    }

    pub fn invalid(mut self, invalid: bool) -> Self {
        self.invalid = invalid;
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// 고정 폭(px). 미지정 시 가용 폭을 채운다.
    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    /// leading 아이콘 painter(15px, text-muted 색으로 호출됨).
    pub fn icon(mut self, icon: IconPainter<'a>) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn addon(mut self, addon: &'a str) -> Self {
        self.addon = Some(addon);
        self
    }

    /// 그리고 TextEdit 응답을 반환한다(`response.changed()` 로 변경 감지).
    pub fn show(self, ui: &mut egui::Ui, theme: &Theme, buf: &mut String) -> egui::Response {
        let height = theme.item_height_interactive.value();
        let pad_x = theme.spacing_md.value();
        let gap = theme.spacing_sm.value();
        let radius = theme.corner_radius.value();
        let bw = theme.border_width.value();
        let body = theme.font_size_body.value();
        let caption = theme.font_size_caption.value();

        let width = self.width.unwrap_or_else(|| ui.available_width());
        let (outer, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());

        // bg.
        ui.painter()
            .rect_filled(outer, radius, theme.surface_raised().to_egui());

        let inner = outer.shrink2(egui::vec2(pad_x, 0.0));
        let inner_w = inner.width();

        // 폭 분배: leading 아이콘 + TextEdit(flex) + trailing addon.
        let icon_glyph = tokens::INPUT_ICON_GLYPH;
        let icon_w = if self.icon.is_some() {
            icon_glyph + gap
        } else {
            0.0
        };
        let addon_font = egui::FontId::monospace(caption);
        let addon_galley = self.addon.map(|a| {
            ui.painter().layout_no_wrap(
                a.to_owned(),
                addon_font.clone(),
                egui::Color32::PLACEHOLDER,
            )
        });
        let addon_w = addon_galley
            .as_ref()
            .map(|g| g.rect.width() + gap)
            .unwrap_or(0.0);
        let te_w = (inner_w - icon_w - addon_w).max(0.0);

        let muted = theme.subtext0.to_egui();
        let resp = ui
            .allocate_new_ui(
                egui::UiBuilder::new()
                    .max_rect(inner)
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
                |ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    if let Some(paint) = self.icon {
                        let (irect, _) = ui.allocate_exact_size(
                            egui::vec2(icon_glyph, icon_glyph),
                            egui::Sense::hover(),
                        );
                        paint(ui, irect, muted);
                    }
                    let font = if self.mono {
                        egui::FontId::monospace(caption)
                    } else {
                        egui::FontId::proportional(body)
                    };
                    let te = egui::TextEdit::singleline(buf)
                        .frame(false)
                        .desired_width(te_w)
                        .hint_text(tasty_egui_theme::hint_text(theme, self.placeholder))
                        .font(font)
                        .text_color(theme.text_primary().to_egui());
                    let r = ui.add_enabled(self.enabled, te);
                    if let Some(g) = addon_galley {
                        let (arect, _) =
                            ui.allocate_exact_size(g.rect.size(), egui::Sense::hover());
                        ui.painter().galley(arect.min, g, muted);
                    }
                    r
                },
            )
            .inner;

        // border (기능 → 즉시, fade 없음).
        let border = if self.invalid {
            theme.accent_danger().to_egui()
        } else if resp.has_focus() {
            theme.border_focus().to_egui()
        } else {
            theme.border_default().to_egui()
        };
        ui.painter().rect_stroke(
            outer,
            radius,
            egui::Stroke::new(bw, border),
            egui::StrokeKind::Inside,
        );
        // focus ring (box-shadow 0 0 0 1px 대체) — 즉시.
        if resp.has_focus() {
            let ring = if self.invalid {
                theme.accent_danger().to_egui()
            } else {
                theme.border_focus().to_egui()
            };
            ui.painter().rect_stroke(
                outer.expand(bw),
                radius,
                egui::Stroke::new(bw, ring),
                egui::StrokeKind::Outside,
            );
        }

        resp
    }
}
