//! `IconButton` — 정사각 아이콘 전용 컨트롤 (디자인 `components/core/IconButton`).
//!
//! 디자인 계약:
//! - border 1px **transparent**(비가시) — ghost 기본. tasty 가 팝업마다 egui 기본
//!   `ui.button` 프레임을 노출하던 버그(예: port_scanner X 버튼 테두리)를 제거한다.
//! - size md(28) / sm(24), glyph 16 / 14, radius `corner_radius`.
//! - ghost: fg `text-secondary` → hover `text-primary`, hover bg `overlay-hover`.
//! - solid: bg `surface-raised` + border `border-default` + fg `text-primary`.
//! - active(지속 선택): fg `accent-primary` + bg `overlay-active`.
//! - disabled: opacity 0.5 (`--tasty-opacity-disabled`).
//!
//! Motion(디자인 .prompt.md): hover 틴트 fade 는 장식 → 스냅 OK. `active` 선택
//! 틴트와 focus-ring 은 기능 → 즉시(여기선 즉시 그린다).
//!
//! 아이콘 시스템은 **호출측 소유** — 본체 `icons::Icon`, 갤러리 mock 모두
//! [`IconPainter`] 클로저로 주입한다(이 crate 는 본체 icons 에 의존하지 않는다).

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
        let radius = theme.corner_radius.value();

        // solid: 채움 + 1px border (transparent 가 아닌 유일한 variant).
        if self.variant == IconButtonVariant::Solid {
            ui.painter().rect(
                rect,
                radius,
                theme.surface_raised().to_egui(),
                egui::Stroke::new(theme.border_width.value(), theme.border_default().to_egui()),
                egui::StrokeKind::Inside,
            );
        }

        // 배경 오버레이. active(지속) 또는 pressed → active 틴트, hover → hover 틴트.
        if self.active || (self.enabled && resp.is_pointer_button_down_on()) {
            ui.painter()
                .rect_filled(rect, radius, theme.overlay_active().to_egui_premultiplied());
        } else if self.enabled && resp.hovered() {
            ui.painter()
                .rect_filled(rect, radius, theme.overlay_hover().to_egui_premultiplied());
        }

        // 글리프 색.
        let color = if self.active {
            theme.accent_primary().to_egui()
        } else if self.variant == IconButtonVariant::Solid || (self.enabled && resp.hovered()) {
            theme.text_primary().to_egui()
        } else {
            theme.text_secondary().to_egui()
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
