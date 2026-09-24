//! 정사각 아이콘 버튼. 투명한 ghost와 채운 solid 형태를 제공한다.
//! 선택·호버·누름 상태를 즉시 표시하며 아이콘은 호출자의 IconPainter로 그린다.

use tasty_type_appearance::theme::Theme;

use crate::control::ControlSize;

/// 디자인 IconButton variant. `ghost`(기본) / `solid`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IconButtonVariant {
    Ghost,
    Solid,
}

/// 글리프 painter: 위젯이 계산한 `rect`(정사각, 중앙) + `color`(상태별 해소)로
/// 아이콘을 그린다. 본체: `|ui, rect, c| icons::CLOSE.image(sz, c).paint_at(ui, rect)`.
pub type IconPainter<'a> = &'a dyn Fn(&mut egui::Ui, egui::Rect, egui::Color32);

/// IconButton 빌더.
pub struct IconButton {
    variant: IconButtonVariant,
    size: ControlSize,
    active: bool,
    enabled: bool,
}

impl Default for IconButton {
    fn default() -> Self {
        Self {
            variant: IconButtonVariant::Ghost,
            size: ControlSize::Md,
            active: false,
            enabled: true,
        }
    }
}

impl IconButton {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn variant(mut self, variant: IconButtonVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn size(mut self, size: ControlSize) -> Self {
        self.size = size;
        self
    }

    /// 지속 선택 상태(engaged tool). accent fg + active overlay.
    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// 그리고 클릭 응답을 반환한다. `paint_icon` 으로 글리프를 주입.
    pub fn show(
        self,
        ui: &mut egui::Ui,
        theme: &Theme,
        paint_icon: IconPainter<'_>,
    ) -> egui::Response {
        let side = self.size.height(theme);
        let sense = if self.enabled {
            egui::Sense::click()
        } else {
            egui::Sense::hover()
        };
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(side, side), sense);
        let radius = theme.icon_button_radius().value();

        // solid 배경과 테두리는 대응 컴포넌트 토큰이 없어 의미별 토큰을 사용한다.
        if self.variant == IconButtonVariant::Solid {
            ui.painter().rect(
                rect,
                radius,
                theme.surface_raised().to_egui(),
                egui::Stroke::new(theme.border_width.value(), theme.border_default().to_egui()),
                egui::StrokeKind::Inside,
            );
        }

        if self.active || (self.enabled && resp.is_pointer_button_down_on()) {
            ui.painter().rect_filled(
                rect,
                radius,
                theme.icon_button_bg_active().to_egui_premultiplied(),
            );
        } else if self.enabled && resp.hovered() {
            ui.painter().rect_filled(
                rect,
                radius,
                theme.icon_button_overlay_hover().to_egui_premultiplied(),
            );
        }

        let color = if self.active {
            theme.accent_primary().to_egui()
        } else if self.variant == IconButtonVariant::Solid || (self.enabled && resp.hovered()) {
            theme.icon_button_fg_hover().to_egui()
        } else {
            theme.icon_button_fg().to_egui()
        };
        let color = if self.enabled {
            color
        } else {
            color.gamma_multiply(theme.opacity_disabled())
        };

        let glyph = self.size.icon_glyph(theme);
        let icon_rect = egui::Rect::from_center_size(rect.center(), egui::vec2(glyph, glyph));
        paint_icon(ui, icon_rect, color);
        resp
    }
}
