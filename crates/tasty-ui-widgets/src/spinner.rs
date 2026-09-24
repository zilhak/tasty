//! 회전하는 호로 진행 중임을 표시한다. 기본 색은 text_muted다.
//! Theme의 모션 감소 설정이 켜지면 회전 대신 정적인 점 세 개를 그린다.

use tasty_type_appearance::theme::Theme;

/// viewBox 24 기준 stroke 두께(디자인 `stroke=2`). 실제 그릴 때 size 비율로 환산.
const VIEWBOX: f32 = 24.0;
const STROKE_VB: f32 = 2.0;
/// track 저대비 alpha (디자인 `.tasty-spinner__track { opacity: 0.22 }`).
const TRACK_ALPHA: f32 = 0.22;
/// arc 가 도는 각도(디자인 `a r r 0 0 1 r r` = 90° 호).
const ARC_SWEEP: f32 = std::f32::consts::FRAC_PI_2;

/// Spinner 빌더.
pub struct Spinner {
    size: Option<f32>,
    /// 설정을 무시하는 override. `None` 이면 `theme.reduced_motion` 을 따른다.
    reduced_motion: Option<bool>,
    /// 호출부 지정 색. `None` 이면 `theme.text_muted()`.
    color: Option<egui::Color32>,
}

impl Default for Spinner {
    fn default() -> Self {
        Self::new()
    }
}

impl Spinner {
    pub fn new() -> Self {
        Self {
            size: None,
            reduced_motion: None,
            color: None,
        }
    }

    /// 정사각 변 길이(px). 미지정이면 아이콘 가족 `icon-size-md`(16).
    pub fn size(mut self, size: f32) -> Self {
        self.size = Some(size);
        self
    }

    /// 모션 감소 설정을 덮어쓴다. 두 상태를 나란히 보여주는 갤러리 등에만 사용한다.
    /// 제품 화면에서는 사용자 접근성 설정을 따라야 한다.
    pub fn reduced_motion(mut self, reduced_motion: bool) -> Self {
        self.reduced_motion = Some(reduced_motion);
        self
    }

    /// arc/track/dot 의 기본 색을 덮어쓴다(미지정 시 `text-muted`).
    pub fn color(mut self, color: egui::Color32) -> Self {
        self.color = Some(color);
        self
    }

    /// 그리고 hover 응답을 반환한다.
    pub fn show(self, ui: &mut egui::Ui, theme: &Theme) -> egui::Response {
        let size = self
            .size
            .unwrap_or_else(|| theme.icon_glyph_size_md.value());
        let color = self.color.unwrap_or_else(|| theme.text_muted().to_egui());
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());

        if !ui.is_rect_visible(rect) {
            return resp;
        }

        if self.reduced_motion.unwrap_or(theme.reduced_motion) {
            draw_dots(ui, rect, color);
            return resp;
        }

        let painter = ui.painter();
        let center = rect.center();
        let scale = size / VIEWBOX;
        let stroke_w = STROKE_VB * scale;
        let radius = (VIEWBOX * 0.5 - STROKE_VB) * scale;

        let track_color = color.gamma_multiply(TRACK_ALPHA);
        painter.circle_stroke(center, radius, egui::Stroke::new(stroke_w, track_color));

        let t = ui.ctx().input(|i| i.time);
        // Theme의 회전 주기를 초 단위로 변환한다.
        let phase = (t / theme.spinner_duration().to_secs_f64()).rem_euclid(1.0) as f32;
        let start = -std::f32::consts::FRAC_PI_2 + phase * std::f32::consts::TAU;
        draw_arc(
            ui,
            center,
            radius,
            start,
            start + ARC_SWEEP,
            stroke_w,
            color,
        );

        ui.ctx().request_repaint();
        resp
    }
}

/// `start_angle`→`end_angle`(라디안, 시계방향 화면 좌표) 사이의 호를 둥근 끝
/// 선분으로 근사해 그린다.
fn draw_arc(
    ui: &mut egui::Ui,
    center: egui::Pos2,
    radius: f32,
    start_angle: f32,
    end_angle: f32,
    stroke_w: f32,
    color: egui::Color32,
) {
    const SEGMENTS: usize = 24;
    let mut points = Vec::with_capacity(SEGMENTS + 1);
    for i in 0..=SEGMENTS {
        let a = start_angle + (end_angle - start_angle) * (i as f32 / SEGMENTS as f32);
        points.push(egui::pos2(
            center.x + radius * a.cos(),
            center.y + radius * a.sin(),
        ));
    }
    let stroke = egui::Stroke::new(stroke_w, color);
    // 둥근 끝(디자인 `stroke-linecap="round"`) 근사 — 양 끝에 반원 캡.
    let cap_r = stroke_w * 0.5;
    if let (Some(&first), Some(&last)) = (points.first(), points.last()) {
        ui.painter().circle_filled(first, cap_r, color);
        ui.painter().circle_filled(last, cap_r, color);
    }
    ui.painter().add(egui::Shape::line(points, stroke));
}

/// reduced-motion fallback — 가로 3-dot (디자인 `radial-gradient` 28% 점 3개).
fn draw_dots(ui: &mut egui::Ui, rect: egui::Rect, color: egui::Color32) {
    let painter = ui.painter();
    // 디자인: 점 지름 ≈ 28% of box → 반경 14%.
    let r = rect.width() * 0.14;
    let cy = rect.center().y;
    let xs = [rect.left() + r, rect.center().x, rect.right() - r];
    for x in xs {
        painter.circle_filled(egui::pos2(x, cy), r, color);
    }
}
