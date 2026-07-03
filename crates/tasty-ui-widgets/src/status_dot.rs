//! `StatusDot` — 상태 점 + 라벨 (디자인 `components/feedback/StatusDot`).
//!
//! dot 8px + gap 6 + caption 라벨(text-secondary). `pulse` 면 확장·페이드 링
//! (scale 0.6→1.8, opacity 0.5→0, 1.6s ease-out). `reduced_motion` 이면 링 생략.
//! tasty Theme 에 status-dot 토큰이 없어 accent-* 로 매핑.

use tasty_type_appearance::theme::Theme;

const GAP: f32 = 6.0;
const RING_INSET: f32 = 3.0; // CSS inset:-3px → base 반경 dot/2 + 3
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
            // Running/Agent/Waiting/Error 는 `status-dot-*` component 색 대응.
            // Idle 은 구현이 text-muted(subtext0 #a6adc8) 를 쓰는데 디자인 status-dot-idle
            // 은 status-idle(#6c7086) 로 불일치 → 픽셀 diff 0 위해 text_muted() 유지.
            // divergence: status-dot 역할이나 대응 role 토큰 부재로 text-muted 로 alias.
            StatusKind::Running => theme.status_dot_success().to_egui(),
            StatusKind::Idle => theme.text_muted().to_egui(),
            StatusKind::Agent => theme.status_dot_agent().to_egui(),
            StatusKind::Waiting => theme.status_dot_warning().to_egui(),
            StatusKind::Error => theme.status_dot_danger().to_egui(),
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
    let dot = theme.status_dot_size().value();
    let caption = theme.font_size_caption.value();
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        egui::FontId::proportional(caption),
        egui::Color32::PLACEHOLDER,
    );
    let h = dot.max(galley.rect.height());
    let w = dot + GAP + galley.rect.width();
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());

    let dot_center = egui::pos2(rect.left() + dot * 0.5, rect.center().y);
    let color = kind.color(theme);

    if pulse && !reduced_motion {
        let t = ui.ctx().input(|i| i.time);
        let period = theme.status_dot_pulse_ms() as f64 / 1000.0; // ms → s
        let phase = (t / period).rem_euclid(1.0) as f32;
        let eased = 1.0 - (1.0 - phase).powi(3); // ease-out cubic
        let radius = (dot * 0.5 + RING_INSET) * (PULSE_SCALE_MIN + PULSE_SCALE_RANGE * eased);
        let ring = color.gamma_multiply(PULSE_OPACITY * (1.0 - eased));
        ui.painter().circle_filled(dot_center, radius, ring);
        ui.ctx().request_repaint();
    }
    ui.painter().circle_filled(dot_center, dot * 0.5, color);

    let pos = egui::pos2(
        rect.left() + dot + GAP,
        rect.center().y - galley.rect.height() * 0.5,
    );
    ui.painter()
        .galley(pos, galley, theme.text_secondary().to_egui());
    resp
}
