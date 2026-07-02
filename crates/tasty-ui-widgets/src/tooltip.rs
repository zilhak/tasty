//! `Tooltip` — 앵커 hover/focus 시 뜨는 불투명 hover 버블
//! (디자인 `components/feedback/Tooltip`).
//!
//! 디자인 계약:
//! - 불투명 카드: bg=`surface-raised`, 1px `border-strong` 보더, radius=`--tasty-radius`,
//!   shadow=`shadow-popover`. **화살표(꼬리) 없음.**
//! - 텍스트: `text-secondary`, `font-size-caption`(11), `line-height-ui`(1.4), 좌측 정렬,
//!   `white-space: normal`(줄바꿈).
//! - padding: y=`space-xs`(4) x=`space-sm`(8). max-width=`tooltip-max-width`(240px).
//!   앵커와의 offset=`space-xs`(4).
//! - placement: top(기본)/bottom/left/right — 앵커 rect 중앙 기준.
//!
//! egui 기본 tooltip(`on_hover_text`)의 전역 스타일/delay 를 건드리지 않기 위해
//! 커스텀 위젯으로 그린다. 표시 여부(hover delay 등)는 호출부([`crate::HelpHint`])가
//! 판정하고, 이 위젯은 넘겨받은 `anchor` rect 기준으로 버블을 **그리기만** 한다
//! (specimen 의 강제 open 도 같은 경로).

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

    /// `anchor` rect 를 기준으로 버블을 그린다(강제 표시). hover/delay 판정은 호출부 몫.
    pub fn show(self, ui: &egui::Ui, theme: &Theme, anchor: egui::Rect) {
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

        // 텍스트는 border-box 240 을 넘지 않도록 padding(x=space-sm ×2)을 뺀 폭에서 wrap.
        let pad_x = theme.spacing_sm.value();
        let pad_y = theme.spacing_xs.value();
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
                // line-height-ui(1.4) = 절대 줄 높이(px) = caption × 1.4.
                line_height: Some(caption * theme.line_height_ui),
                ..Default::default()
            },
        );

        egui::Area::new(self.id)
            .order(egui::Order::Tooltip)
            .fixed_pos(anchor_pos)
            .pivot(pivot)
            .constrain(true) // 화면/모달 밖으로 나가면 egui 기본 constrain 이 안으로 당김.
            .interactable(false)
            .show(ui.ctx(), |ui| {
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
}
