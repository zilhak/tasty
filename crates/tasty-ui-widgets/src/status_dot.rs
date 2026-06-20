//! `StatusDot` — 상태 점 + 라벨 (디자인 `components/feedback/StatusDot`).
//!
//! dot 8px + gap 6 + caption 라벨(text-secondary). `pulse` 면 확장·페이드 링
//! (scale 0.6→1.8, opacity 0.5→0, 1.6s ease-out). `reduced_motion` 이면 링 생략.
//! tasty Theme 에 status-dot 토큰이 없어 accent-* 로 매핑.

use tasty_type_appearance::theme::Theme;

const DOT: f32 = 8.0;
const GAP: f32 = 6.0;
const RING_INSET: f32 = 3.0; // CSS inset:-3px → base 반경 dot/2 + 3
const PULSE_PERIOD: f64 = 1.6;
const PULSE_SCALE_MIN: f32 = 0.6;
const PULSE_SCALE_RANGE: f32 = 1.2;
const PULSE_OPACITY: f32 = 0.5;

/// 디자인 StatusDot status.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum StatusKind {
    Running,
    Idle,
    Agent,
    Waiting,
    Error,
}

impl StatusKind {
    fn color(self, theme: &Theme) -> egui::Color32 {
        match self {
            StatusKind::Running => theme.accent_success().to_egui(),
            StatusKind::Idle => theme.subtext0.to_egui(),
            StatusKind::Agent => theme.accent_agent().to_egui(),
            StatusKind::Waiting => theme.accent_warning().to_egui(),
            StatusKind::Error => theme.accent_danger().to_egui(),
        }
    }
}

/// 상태 점 + 라벨을 한 줄로 그린다. `pulse` + `!reduced_motion` 이면 링 애니메이션.
pub fn status_dot(
    ui: &mut egui::Ui,
    theme: &Theme,
    kind: StatusKind,
    label: &str,
    pulse: bool,
    reduced_motion: bool,
) -> egui::Response {
    let caption = theme.font_size_caption.value();
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        egui::FontId::proportional(caption),
        egui::Color32::PLACEHOLDER,
    );
    let h = DOT.max(galley.rect.height());
    let w = DOT + GAP + galley.rect.width();
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());

    let dot_center = egui::pos2(rect.left() + DOT * 0.5, rect.center().y);
    let color = kind.color(theme);

    if pulse && !reduced_motion {
        let t = ui.ctx().input(|i| i.time);
        let phase = (t / PULSE_PERIOD).rem_euclid(1.0) as f32;
        let eased = 1.0 - (1.0 - phase).powi(3); // ease-out cubic
        let radius = (DOT * 0.5 + RING_INSET) * (PULSE_SCALE_MIN + PULSE_SCALE_RANGE * eased);
        let ring = color.gamma_multiply(PULSE_OPACITY * (1.0 - eased));
        ui.painter().circle_filled(dot_center, radius, ring);
        ui.ctx().request_repaint();
    }
    ui.painter().circle_filled(dot_center, DOT * 0.5, color);

    let pos = egui::pos2(
        rect.left() + DOT + GAP,
        rect.center().y - galley.rect.height() * 0.5,
    );
    ui.painter()
        .galley(pos, galley, theme.text_secondary().to_egui());
    resp
}
