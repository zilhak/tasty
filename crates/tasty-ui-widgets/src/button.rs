//! Theme 토큰으로 채움·외곽선·투명 버튼을 그린다.
//! 호버와 누름 상태는 애니메이션 없이 바로 반영한다.
//! 별도 굵은 글꼴을 등록하지 않으므로 글꼴 굵기 대신 색으로 강조를 구분한다.
//! disabled는 opacity를 곱하지 않는다. 모든 variant가 같은 중립 상자와 disabled ink로 그려진다.

use tasty_type_appearance::theme::Theme;

use crate::control::ControlSize;
use crate::icon_button::IconPainter;

/// 디자인 Button variant.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ButtonVariant {
    Primary,
    Secondary,
    Ghost,
    Danger,
    Agent,
}

/// 배너 셸 안이라는 문맥을 기록하는 egui 임시 데이터 키. 값은 중첩 깊이다.
fn banner_surface_id() -> egui::Id {
    egui::Id::new("tasty_ui_widgets::button::banner_surface")
}

/// `content`를 배너 셸 문맥(시안 `data-surface="banner"`)에서 그린다.
/// 이 안의 Secondary 버튼은 배너 배경보다 한 단계 위의 채움과 테두리
/// (`banner_button_bg`·`banner_button_border`)를 쓴다. `banner_shell`이 모든 배너에 적용한다.
pub fn banner_surface<R>(ui: &mut egui::Ui, content: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let id = banner_surface_id();
    let prev = ui.ctx().data(|d| d.get_temp::<u32>(id)).unwrap_or(0);
    ui.ctx().data_mut(|d| d.insert_temp(id, prev + 1));
    let out = content(ui);
    ui.ctx().data_mut(|d| d.insert_temp(id, prev));
    out
}

/// 지금 그리는 위치가 배너 셸 문맥 안인지.
fn on_banner_surface(ui: &egui::Ui) -> bool {
    ui.ctx()
        .data(|d| d.get_temp::<u32>(banner_surface_id()))
        .unwrap_or(0)
        > 0
}

/// Button 빌더.
pub struct Button<'a> {
    label: &'a str,
    variant: ButtonVariant,
    size: ControlSize,
    enabled: bool,
    /// 컨테이너 폭을 채운다(디자인 `block`).
    block: bool,
    /// 라벨 앞 leading 아이콘(디자인 `leadingIcon`). icon-size-md, fg 색으로 그려짐.
    leading_icon: Option<IconPainter<'a>>,
    /// 라벨 뒤 trailing 아이콘(디자인 `trailingIcon`). icon-size-md, fg 색으로 그려짐.
    trailing_icon: Option<IconPainter<'a>>,
}

impl<'a> Button<'a> {
    pub fn new(label: &'a str) -> Self {
        Self {
            label,
            variant: ButtonVariant::Primary,
            size: ControlSize::Md,
            enabled: true,
            block: false,
            leading_icon: None,
            trailing_icon: None,
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

    /// 라벨 앞 leading 아이콘(디자인 `leadingIcon`).
    pub fn leading_icon(mut self, icon: IconPainter<'a>) -> Self {
        self.leading_icon = Some(icon);
        self
    }

    /// 라벨 뒤 trailing 아이콘(디자인 `trailingIcon`).
    pub fn trailing_icon(mut self, icon: IconPainter<'a>) -> Self {
        self.trailing_icon = Some(icon);
        self
    }

    pub fn show(self, ui: &mut egui::Ui, theme: &Theme) -> egui::Response {
        let height = self.size.height(theme);
        let pad_x = self.size.pad_x(theme);
        let radius = theme.button_radius().value();
        let bw = theme.border_width.value();
        let icon_glyph = theme.icon_glyph_size_md.value();
        let gap = theme.button_gap().value();
        let has_leading = self.leading_icon.is_some();
        let has_trailing = self.trailing_icon.is_some();

        // 글자를 배치한 뒤 상태별 색을 지정한다.
        let font_id = egui::FontId::proportional(self.size.font_size(theme));
        let galley =
            ui.painter()
                .layout_no_wrap(self.label.to_owned(), font_id, egui::Color32::PLACEHOLDER);

        let icons_w = (if has_leading { icon_glyph + gap } else { 0.0 })
            + (if has_trailing { icon_glyph + gap } else { 0.0 });
        let content_w = galley.rect.size().x + icons_w + 2.0 * pad_x;
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

        let (fill, border, fg) = if !self.enabled {
            // accent 채움은 빠지고 모든 variant가 같은 중립 상자를 쓴다. Ghost는 상자가 없다.
            let fg = theme.button_disabled_fg().to_egui();
            match self.variant {
                ButtonVariant::Ghost => (None, None, fg),
                _ => (
                    Some(theme.button_disabled_bg().to_egui()),
                    Some(theme.button_disabled_border().to_egui()),
                    fg,
                ),
            }
        } else {
            match self.variant {
                ButtonVariant::Primary => (
                    Some(theme.button_primary_bg().to_egui()),
                    None,
                    theme.button_primary_fg().to_egui(),
                ),
                ButtonVariant::Agent => (
                    Some(theme.button_agent_bg().to_egui()),
                    None,
                    theme.button_agent_fg().to_egui(),
                ),
                ButtonVariant::Danger => (
                    Some(theme.button_danger_bg().to_egui()),
                    None,
                    theme.button_danger_fg().to_egui(),
                ),
                // 배너 위에서는 채움과 테두리가 배너 배경보다 한 단계 위이며 hover에도 테두리가 그대로다.
                ButtonVariant::Secondary if on_banner_surface(ui) => (
                    Some(theme.banner_button_bg().to_egui()),
                    Some(theme.banner_button_border().to_egui()),
                    theme.button_fg().to_egui(),
                ),
                ButtonVariant::Secondary => {
                    let b = if resp.hovered() {
                        theme.button_secondary_border_hover()
                    } else {
                        theme.button_secondary_border()
                    };
                    (
                        Some(theme.button_secondary_bg().to_egui()),
                        Some(b.to_egui()),
                        theme.button_fg().to_egui(),
                    )
                }
                ButtonVariant::Ghost => {
                    let f = if resp.hovered() {
                        theme.button_ghost_fg_hover()
                    } else {
                        theme.button_ghost_fg()
                    };
                    (None, None, f.to_egui())
                }
            }
        };

        if let Some(f) = fill {
            ui.painter().rect_filled(rect, radius, f);
        }
        if let Some(b) = border {
            ui.painter().rect_stroke(
                rect,
                radius,
                egui::Stroke::new(bw, b),
                egui::StrokeKind::Inside,
            );
        }
        if self.enabled {
            if resp.is_pointer_button_down_on() {
                ui.painter().rect_filled(
                    rect,
                    radius,
                    theme.button_overlay_active().to_egui_premultiplied(),
                );
            } else if resp.hovered() {
                ui.painter().rect_filled(
                    rect,
                    radius,
                    theme.button_overlay_hover().to_egui_premultiplied(),
                );
            }
        }

        let label_w = galley.rect.size().x;
        let group_w = label_w
            + (if has_leading { icon_glyph + gap } else { 0.0 })
            + (if has_trailing { icon_glyph + gap } else { 0.0 });
        let mut x = rect.center().x - group_w * 0.5;
        let cy = rect.center().y;
        let fg_col = fg;

        if let Some(paint) = self.leading_icon {
            let irect = egui::Rect::from_center_size(
                egui::pos2(x + icon_glyph * 0.5, cy),
                egui::vec2(icon_glyph, icon_glyph),
            );
            paint(ui, irect, fg_col);
            x += icon_glyph + gap;
        }
        let text_pos = egui::pos2(x, cy - galley.rect.size().y * 0.5);
        ui.painter().galley(text_pos, galley, fg_col);
        x += label_w;
        if let Some(paint) = self.trailing_icon {
            x += gap;
            let irect = egui::Rect::from_center_size(
                egui::pos2(x + icon_glyph * 0.5, cy),
                egui::vec2(icon_glyph, icon_glyph),
            );
            paint(ui, irect, fg_col);
        }

        resp
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn banner_surface_marks_only_its_own_scope_and_nests() {
        let ctx = egui::Context::default();
        let _output = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                assert!(!on_banner_surface(ui));
                banner_surface(ui, |ui| {
                    assert!(on_banner_surface(ui));
                    banner_surface(ui, |ui| assert!(on_banner_surface(ui)));
                    assert!(on_banner_surface(ui));
                });
                assert!(!on_banner_surface(ui));
            });
        });
    }
}
