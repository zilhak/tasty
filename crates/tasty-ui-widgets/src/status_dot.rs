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
///
/// **라벨이 비면 점만 그리고 폭도 점 하나뿐이다.** 라벨 없는 점이 필요한 자리(행
/// 좌측의 실행 표시 등)가 이 위젯을 그대로 부를 수 있어야 하는데, 뒤따르는 라벨이
/// 없는데도 `GAP` 을 할당하면 그 자리만 정렬선이 밀린다. 소비자가 폭을 되빼는 래퍼를
/// 쓰게 만들지 않는다 — 되빼는 값은 그때마다 다시 손으로 적히고 여기 상수와 갈린다.
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
    // 라벨이 없으면 gap 도 없다 — 뒤에 붙을 것이 없는 여백이다.
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
