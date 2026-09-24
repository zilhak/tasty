//! 상태 점과 선택적인 라벨. 색상과 점 크기는 Theme의 상태 점 토큰을 사용한다.
//! pulse가 켜지고 reduced_motion이 꺼진 경우에만 확장·페이드 링을 그린다.

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
            StatusKind::Running => theme.status_dot_success().to_egui(),
            StatusKind::Idle => theme.status_dot_idle().to_egui(),
            StatusKind::Agent => theme.status_dot_agent().to_egui(),
            StatusKind::Waiting => theme.status_dot_warning().to_egui(),
            StatusKind::Error => theme.status_dot_danger().to_egui(),
        }
    }
}

/// 상태 점과 라벨을 그린다. 라벨이 비면 간격 없이 점 하나의 폭만 사용한다.
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
    let w = dot
        + if label.is_empty() {
            0.0
        } else {
            GAP + galley.rect.width()
        };
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());

    let dot_center = egui::pos2(rect.left() + dot * 0.5, rect.center().y);
    let color = kind.color(theme);

    if pulse && !reduced_motion {
        let t = ui.ctx().input(|i| i.time);
        let period = theme.status_dot_pulse_duration().to_secs_f64();
        let phase = (t / period).rem_euclid(1.0) as f32;
        let eased = 1.0 - (1.0 - phase).powi(3); // ease-out cubic
        let radius = (dot * 0.5 + RING_INSET) * (PULSE_SCALE_MIN + PULSE_SCALE_RANGE * eased);
        let ring = color.gamma_multiply(PULSE_OPACITY * (1.0 - eased));
        ui.painter().circle_filled(dot_center, radius, ring);
        ui.ctx().request_repaint();
    }
    ui.painter().circle_filled(dot_center, dot * 0.5, color);

    if !label.is_empty() {
        let pos = egui::pos2(
            rect.left() + dot + GAP,
            rect.center().y - galley.rect.height() * 0.5,
        );
        ui.painter()
            .galley(pos, galley, theme.text_secondary().to_egui());
    }
    resp
}
