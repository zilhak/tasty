//! `Spinner` — 짧은 백그라운드 작업용 비결정형 진행 표시
//! (디자인 `components/feedback/Spinner`).
//!
//! 디자인 계약:
//! - 기본 16px(디자인 `--tasty-size-16`), 얇은 회전 arc + 저대비 track.
//! - track 은 `text-muted` 색 + opacity 0.22, arc 는 같은 색 opacity 1.
//! - viewBox 24 기준 stroke 2 → 반경 `12 - stroke`. arc 는 상단에서 시작하는 90° 호.
//! - 회전: 0.9s linear infinite (디자인 `tasty-spin`).
//! - `prefers-reduced-motion`: 회전 정지 + 3-dot 정적 표시(디자인 fallback).
//!
//! egui 에는 `prefers-reduced-motion` 매체 질의가 없으므로 tasty 는 이 값을
//! `Theme` 에 실어 나른다 — 스피너는 **기본으로 `theme.reduced_motion` 을 읽는다.**
//! 종전에는 호출부가 명시적으로 넘겨야 했고, 실제로 넘기는 자리가 레포 전체에
//! 하나도 없어서 설정을 켜도 스피너가 계속 돌았다. 결정은
//! `docs/adr/0174-theme-carries-reduced-motion.md`. 색은 호출부 지정이 없으면
//! `theme.text_muted()` 를 쓴다.

use tasty_type_appearance::theme::Theme;

/// 디자인 기본 크기 = `--tasty-size-16`.
const DEFAULT_SIZE: f32 = 16.0;
/// viewBox 24 기준 stroke 두께(디자인 `stroke=2`). 실제 그릴 때 size 비율로 환산.
const VIEWBOX: f32 = 24.0;
const STROKE_VB: f32 = 2.0;
/// track 저대비 alpha (디자인 `.tasty-spinner__track { opacity: 0.22 }`).
const TRACK_ALPHA: f32 = 0.22;
/// 회전 주기(초) — 디자인 `animation: tasty-spin 0.9s linear infinite`.
const SPIN_PERIOD: f64 = 0.9;
/// arc 가 도는 각도(디자인 `a r r 0 0 1 r r` = 90° 호).
const ARC_SWEEP: f32 = std::f32::consts::FRAC_PI_2;

/// Spinner 빌더.
pub struct Spinner {
    size: f32,
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
            size: DEFAULT_SIZE,
            reduced_motion: None,
            color: None,
        }
    }

    /// 정사각 변 길이(px). 기본 16.
    pub fn size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }

    /// 사용자 설정(`theme.reduced_motion`)을 **무시하고** 모션 여부를 고정한다.
    ///
    /// 이건 예외 전용이다 — 실행 중인 설정과 무관하게 두 상태를 **동시에 보여야
    /// 하는** 자리에만 쓴다(갤러리 specimen 이 유일한 그런 자리다). 제품 화면에서
    /// 이걸 부르면 접근성 설정을 되돌리는 것이므로, 새로 부르기 전에 그 자리가
    /// 정말 "설정을 무시해야 하는" 자리인지 먼저 답해라.
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
        let size = self.size;
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
        // viewBox 24 → 실제 size 로 스케일.
        let scale = size / VIEWBOX;
        let stroke_w = STROKE_VB * scale;
        let radius = (VIEWBOX * 0.5 - STROKE_VB) * scale;

        // track — 전체 원, 저대비.
        let track_color = color.gamma_multiply(TRACK_ALPHA);
        painter.circle_stroke(center, radius, egui::Stroke::new(stroke_w, track_color));

        // arc — 상단에서 시작하는 90° 호, 시간에 따라 회전.
        let t = ui.ctx().input(|i| i.time);
        let phase = (t / SPIN_PERIOD).rem_euclid(1.0) as f32;
        // -90°(상단) 기준 시작 + 회전 위상.
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
