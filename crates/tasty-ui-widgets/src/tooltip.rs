//! 앵커 주변에 도움말 버블을 그린다. 위치는 위·아래·왼쪽·오른쪽 중에서 선택한다.
//! 호버와 대기 시간은 호출자가 판단하며 egui 전역 도움말 설정은 바꾸지 않는다.

use tasty_type_appearance::theme::Theme;

/// 버블이 앵커의 어느 쪽에 뜨는지 — 앵커 rect 중앙 기준.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TooltipPlacement {
    /// 앵커 위(기본).
    #[default]
    Top,
    /// 앵커 아래.
    Bottom,
    /// 앵커 왼쪽.
    Left,
    /// 앵커 오른쪽.
    Right,
}

/// Tooltip 버블 빌더.
pub struct Tooltip<'a> {
    text: &'a str,
    placement: TooltipPlacement,
    /// 버블 `Area` 의 고유 id — 한 프레임에 여러 버블(specimen 4 placement)을 그릴 때
    /// 충돌을 막는다. 기본값은 단일 사용을 가정한 고정 id.
    id: egui::Id,
}

impl<'a> Tooltip<'a> {
    pub fn new(text: &'a str) -> Self {
        Self {
            text,
            placement: TooltipPlacement::default(),
            id: egui::Id::new("tasty_tooltip"),
        }
    }

    /// 앵커 기준 배치.
    pub fn placement(mut self, placement: TooltipPlacement) -> Self {
        self.placement = placement;
        self
    }

    /// 버블 `Area` id 의 출처 — 한 페이지에 여러 버블을 동시에 그릴 때 고유화한다.
    pub fn id_source(mut self, source: impl std::hash::Hash) -> Self {
        self.id = egui::Id::new(source);
        self
    }

    /// 위에 들어가면 위, 아니면 아래에 들어가면 아래, 둘 다 안 되면 위(창 안으로 당김)로 배치한다.
    /// `bounds`는 버블이 들어가야 하는 영역(보통 창 전체)이다.
    pub fn placement_top_then_bottom(
        self,
        ctx: &egui::Context,
        theme: &Theme,
        anchor: egui::Rect,
        bounds: egui::Rect,
    ) -> Self {
        let offset = theme.spacing_xs.value();
        let height = self.bubble_height(ctx, theme);
        let placement = if anchor.top() - offset - height >= bounds.top()
            || anchor.bottom() + offset + height > bounds.bottom()
        {
            TooltipPlacement::Top
        } else {
            TooltipPlacement::Bottom
        };
        self.placement(placement)
    }

    /// `anchor` rect 를 기준으로 버블을 그린다(강제 표시). hover/delay 판정은 호출부 몫.
    pub fn show(self, ui: &egui::Ui, theme: &Theme, anchor: egui::Rect) {
        self.show_in(ui.ctx(), theme, anchor);
    }

    /// `Ui` 없이 painter만 쓰는 호출부용 [`Tooltip::show`].
    pub fn show_in(self, ctx: &egui::Context, theme: &Theme, anchor: egui::Rect) {
        let offset = theme.spacing_xs.value();
        // 앵커 rect 중앙 기준 앵커 포인트 + 버블 pivot(버블에서 앵커에 붙는 변).
        let (anchor_pos, pivot) = match self.placement {
            TooltipPlacement::Top => (
                egui::pos2(anchor.center().x, anchor.top() - offset),
                egui::Align2::CENTER_BOTTOM,
            ),
            TooltipPlacement::Bottom => (
                egui::pos2(anchor.center().x, anchor.bottom() + offset),
                egui::Align2::CENTER_TOP,
            ),
            TooltipPlacement::Left => (
                egui::pos2(anchor.left() - offset, anchor.center().y),
                egui::Align2::RIGHT_CENTER,
            ),
            TooltipPlacement::Right => (
                egui::pos2(anchor.right() + offset, anchor.center().y),
                egui::Align2::LEFT_CENTER,
            ),
        };

        let pad_x = theme.spacing_sm.value();
        let pad_y = theme.spacing_xs.value();
        let job = self.layout_job(theme);

        egui::Area::new(self.id)
            .order(egui::Order::Tooltip)
            .fixed_pos(anchor_pos)
            .pivot(pivot)
            .constrain(true) // 화면/모달 밖으로 나가면 egui 기본 constrain 이 안으로 당김.
            .interactable(false)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(theme.surface_raised().to_egui())
                    .stroke(egui::Stroke::new(
                        theme.border_width.value(),
                        theme.border_strong().to_egui(),
                    ))
                    .corner_radius(theme.corner_radius.value())
                    .shadow(theme.shadow_popover().to_egui())
                    .inner_margin(egui::Margin::symmetric(pad_x as i8, pad_y as i8))
                    .show(ui, |ui| {
                        ui.set_max_width(theme.tooltip_max_width.value());
                        ui.label(job);
                    });
            });
    }

    fn layout_job(&self, theme: &Theme) -> egui::text::LayoutJob {
        // 텍스트는 border-box 240 을 넘지 않도록 padding(x=space-sm ×2)을 뺀 폭에서 wrap.
        let pad_x = theme.spacing_sm.value();
        let text_wrap = (theme.tooltip_max_width.value() - pad_x * 2.0).max(0.0);

        let caption = theme.font_size_caption.value();
        let mut job = egui::text::LayoutJob {
            halign: egui::Align::LEFT,
            ..Default::default()
        };
        job.wrap.max_width = text_wrap;
        job.append(
            self.text,
            0.0,
            egui::TextFormat {
                font_id: egui::FontId::proportional(caption),
                color: theme.text_secondary().to_egui(),
                // 줄 높이는 caption 글꼴 크기에 line_height_ui를 곱한다.
                line_height: Some(caption * theme.line_height_ui),
                ..Default::default()
            },
        );
        job
    }

    /// 버블 높이 — 텍스트 + 위아래 padding + 위아래 테두리.
    fn bubble_height(&self, ctx: &egui::Context, theme: &Theme) -> f32 {
        let text = ctx.fonts(|f| f.layout_job(self.layout_job(theme)).size().y);
        text + theme.spacing_xs.scaled(2.0).value() + theme.border_width.scaled(2.0).value()
    }
}

/// `hovered`가 이어진 시간이 `tooltip-delay`를 넘었는지 판정한다. 벗어나면 초기화한다.
/// 다른 egui 도움말의 대기 시간을 바꾸지 않도록 `id`별 자체 타이머를 사용한다.
pub fn tooltip_hover_delay_elapsed(
    ctx: &egui::Context,
    theme: &Theme,
    id: egui::Id,
    hovered: bool,
) -> bool {
    let key = id.with("tooltip_hover_started_at");
    if hovered {
        let now = ctx.input(|i| i.time);
        let start = ctx.data_mut(|d| *d.get_temp_mut_or_insert_with(key, || now));
        if now - start < theme.tooltip_delay().to_secs_f64() {
            // delay 경과 후 자동으로 다시 판정되도록 repaint 예약.
            ctx.request_repaint();
            false
        } else {
            true
        }
    } else {
        ctx.data_mut(|d| d.remove::<f64>(key));
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn theme() -> Theme {
        Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0)
    }

    /// 창 안의 한 줄 앵커가 받는 배치.
    fn placement_for(anchor_top: f32) -> TooltipPlacement {
        let bounds = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        let anchor =
            egui::Rect::from_min_size(egui::pos2(100.0, anchor_top), egui::vec2(200.0, 28.0));
        let ctx = egui::Context::default();
        let mut placement = TooltipPlacement::Left;
        let _output = ctx.run(egui::RawInput::default(), |ctx| {
            placement = Tooltip::new("Surface Type")
                .placement_top_then_bottom(ctx, &theme(), anchor, bounds)
                .placement;
        });
        placement
    }

    #[test]
    fn top_is_used_when_the_bubble_fits_above() {
        assert_eq!(placement_for(300.0), TooltipPlacement::Top);
    }

    #[test]
    fn bottom_is_used_when_the_bubble_does_not_fit_above() {
        assert_eq!(placement_for(0.0), TooltipPlacement::Bottom);
    }

    #[test]
    fn top_is_kept_when_neither_side_fits() {
        let bounds = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 30.0));
        let anchor = egui::Rect::from_min_size(egui::pos2(100.0, 1.0), egui::vec2(200.0, 28.0));
        let ctx = egui::Context::default();
        let _output = ctx.run(egui::RawInput::default(), |ctx| {
            let t = Tooltip::new("Surface Type").placement_top_then_bottom(
                ctx,
                &theme(),
                anchor,
                bounds,
            );
            assert_eq!(t.placement, TooltipPlacement::Top);
        });
    }
}
