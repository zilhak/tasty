//! 라벨 옆 물음표와 호버 도움말. 클릭 동작 없이 Tooltip을 조합한다.
//! 아이콘은 painter로 직접 그리며 tasty-icons의 HELP_CIRCLE SVG와는 별도 구현이다.

use tasty_type_appearance::theme::Theme;

use crate::tooltip::{Tooltip, TooltipPlacement};

/// HelpHint 빌더.
pub struct HelpHint<'a> {
    text: &'a str,
    placement: TooltipPlacement,
    /// 강제 표시(specimen 의 open prop) — hover 없이 버블을 띄운다.
    open: bool,
    /// 버블 `Area` id 출처 — 한 페이지에 여러 개(placement 4종)를 동시에 그릴 때 고유화.
    id: Option<egui::Id>,
}

impl<'a> HelpHint<'a> {
    pub fn new(text: &'a str) -> Self {
        Self {
            text,
            placement: TooltipPlacement::default(),
            open: false,
            id: None,
        }
    }

    /// 버블 배치(앵커 기준).
    pub fn placement(mut self, placement: TooltipPlacement) -> Self {
        self.placement = placement;
        self
    }

    /// 강제 표시(specimen). hover 여부와 무관하게 버블을 그린다.
    pub fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }

    /// 버블 `Area` id 출처를 명시(specimen 에서 여러 버블 동시 표시 시 충돌 방지).
    pub fn id_source(mut self, source: impl std::hash::Hash) -> Self {
        self.id = Some(egui::Id::new(source));
        self
    }

    /// 글리프를 그리고 hover(또는 강제 open) 시 Tooltip 을 조합해 표시한다.
    pub fn show(self, ui: &mut egui::Ui, theme: &Theme) -> egui::Response {
        let size = theme.icon_glyph_size_sm.value();
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
        let hovered = resp.hovered();

        if ui.is_rect_visible(rect) {
            let color = if hovered || self.open {
                theme.text_secondary()
            } else {
                theme.text_muted()
            }
            .to_egui();
            paint_help_glyph(ui.painter(), rect, color);
        }

        let resp = resp.on_hover_cursor(egui::CursorIcon::Help);

        let show = self.open || hover_delay_elapsed(ui, theme, resp.id, hovered);
        if show {
            let tip_id = self.id.unwrap_or_else(|| resp.id.with("tooltip"));
            Tooltip::new(self.text)
                .placement(self.placement)
                .id_source(tip_id)
                .show(ui, theme, rect);
        }
        resp
    }
}

/// 호버가 시작된 시각부터 지연을 계산하고 벗어나면 초기화한다.
/// 다른 egui 도움말의 대기 시간을 바꾸지 않도록 자체 타이머를 사용한다.
fn hover_delay_elapsed(ui: &egui::Ui, theme: &Theme, id: egui::Id, hovered: bool) -> bool {
    let key = id.with("help_hint_hover_started_at");
    if hovered {
        let now = ui.ctx().input(|i| i.time);
        let start = ui
            .ctx()
            .data_mut(|d| *d.get_temp_mut_or_insert_with(key, || now));
        if now - start < theme.tooltip_delay().to_secs_f64() {
            // delay 경과 후 자동으로 다시 판정되도록 repaint 예약.
            ui.ctx().request_repaint();
            false
        } else {
            true
        }
    } else {
        ui.ctx().data_mut(|d| d.remove::<f64>(key));
        false
    }
}

/// design 글리프 path 를 painter 로 그린다 — 바깥 원 + 물음표 훅/꼬리(arc+cubic) + 점.
/// `rect` 는 정사각 앵커(변 = size). viewBox 24 를 `rect` 로 스케일한다.
fn paint_help_glyph(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let size = rect.width();
    let scale = size / 24.0;
    let map = |x: f32, y: f32| rect.min + egui::vec2(x * scale, y * scale);
    let stroke_w = 2.0 * scale;
    let stroke = egui::Stroke::new(stroke_w, color);

    // 바깥 원 — `circle cx=12 cy=12 r=10`.
    painter.circle_stroke(map(12.0, 12.0), 10.0 * scale, stroke);

    // 물음표 훅(arc `M9.09 9 a3 3 0 0 1 5.83 1`) → 꼬리(cubic `c0 2 -3 3 -3 3`).
    // SVG 호의 끝점 표현을 중심 좌표로 변환한 값이다.
    let mut pts: Vec<egui::Pos2> = Vec::new();
    let (cx, cy, r) = (11.92_f32, 9.996_f32, 3.0_f32);
    let start = 199.4_f32.to_radians();
    let sweep = 160.7_f32.to_radians();
    const ARC_SEG: usize = 14;
    for i in 0..=ARC_SEG {
        let a = start + sweep * (i as f32 / ARC_SEG as f32);
        pts.push(map(cx + r * a.cos(), cy + r * a.sin()));
    }
    // cubic bezier: 훅 끝 (14.92,10) → 꼬리 (11.92,13).
    let (p0, c1, c2, p3) = (
        (14.92_f32, 10.0_f32),
        (14.92_f32, 12.0_f32),
        (11.92_f32, 13.0_f32),
        (11.92_f32, 13.0_f32),
    );
    const CUB_SEG: usize = 10;
    for i in 1..=CUB_SEG {
        let t = i as f32 / CUB_SEG as f32;
        let u = 1.0 - t;
        let x =
            u * u * u * p0.0 + 3.0 * u * u * t * c1.0 + 3.0 * u * t * t * c2.0 + t * t * t * p3.0;
        let y =
            u * u * u * p0.1 + 3.0 * u * u * t * c1.1 + 3.0 * u * t * t * c2.1 + t * t * t * p3.1;
        pts.push(map(x, y));
    }
    // round cap 근사(spinner 전례) — 양 끝에 반경 stroke/2 원.
    let cap = stroke_w * 0.5;
    if let (Some(&f), Some(&l)) = (pts.first(), pts.last()) {
        painter.circle_filled(f, cap, color);
        painter.circle_filled(l, cap, color);
    }
    painter.add(egui::Shape::line(pts, stroke));

    // 점 — `M12 17 h.01` (round-cap 0-length line = 지름 stroke 의 점).
    painter.circle_filled(map(12.0, 17.0), stroke_w * 0.5, color);
}
