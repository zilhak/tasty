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
        // disabled solid는 disabled 상자 role을 읽는다.
        if self.variant == IconButtonVariant::Solid {
            let (fill, edge) = if self.enabled {
                (theme.surface_raised(), theme.border_default())
            } else {
                (theme.state_disabled_fill(), theme.state_disabled_border())
            };
            ui.painter().rect(
                rect,
                radius,
                fill.to_egui(),
                egui::Stroke::new(theme.border_width.value(), edge.to_egui()),
                egui::StrokeKind::Inside,
            );
        }

        let color = paint_icon_button_state(
            ui.painter(),
            theme,
            rect,
            IconButtonState {
                variant: self.variant,
                enabled: self.enabled,
                active: self.active,
                hovered: resp.hovered(),
                pressed: resp.is_pointer_button_down_on(),
            },
        );

        let glyph = self.size.icon_glyph(theme);
        let icon_rect = egui::Rect::from_center_size(rect.center(), egui::vec2(glyph, glyph));
        paint_icon(ui, icon_rect, color);
        resp
    }
}

/// IconButton 한 칸의 상태. painter만 쓰고 hit-test를 직접 하는 호출부(popup 타이틀바)가
/// 판정 결과를 넘긴다.
#[derive(Clone, Copy, Debug)]
pub struct IconButtonState {
    pub variant: IconButtonVariant,
    pub enabled: bool,
    pub active: bool,
    pub hovered: bool,
    pub pressed: bool,
}

impl IconButtonState {
    /// 사용 가능한 ghost 버튼.
    pub fn ghost(hovered: bool, pressed: bool) -> Self {
        Self {
            variant: IconButtonVariant::Ghost,
            enabled: true,
            active: false,
            hovered,
            pressed,
        }
    }
}

/// IconButton의 active·hover 배경을 `rect`에 칠하고 글리프 색을 반환한다. 위젯과 painter
/// 호출부가 같은 규칙을 쓴다. solid 바탕은 위젯만 그린다.
pub fn paint_icon_button_state(
    painter: &egui::Painter,
    theme: &Theme,
    rect: egui::Rect,
    state: IconButtonState,
) -> egui::Color32 {
    let radius = theme.icon_button_radius().value();
    // disabled는 active·hover 배경을 그리지 않는다.
    if state.enabled && (state.active || state.pressed) {
        painter.rect_filled(
            rect,
            radius,
            theme.icon_button_bg_active().to_egui_premultiplied(),
        );
    } else if state.enabled && state.hovered {
        painter.rect_filled(
            rect,
            radius,
            theme.icon_button_overlay_hover().to_egui_premultiplied(),
        );
    }

    // disabled 글리프는 opacity 없이 disabled ink를 쓴다.
    if !state.enabled {
        theme.state_disabled_fg().to_egui()
    } else if state.active {
        theme.accent_primary().to_egui()
    } else if state.variant == IconButtonVariant::Solid || state.hovered {
        theme.icon_button_fg_hover().to_egui()
    } else {
        theme.icon_button_fg().to_egui()
    }
}
