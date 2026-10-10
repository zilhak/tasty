//! egui 의 지연 repaint 요청(`request_repaint_after`)을 이벤트 루프로 넘길지 정한다.
//!
//! egui 는 한 프레임에서 같은 요청을 여러 번, 남은 시간을 줄여 가며 다시 부른다. 매번 이벤트를
//! 보내면 대기 중인 루프를 그만큼 깨우므로, 아직 오지 않은 더 이른 예약이 있으면 보내지 않는다.
//! 예약 시각이 지나면 다음 요청을 받는다. 발화한 프레임에서 아직 필요한 요청은 egui 가 다시 낸다.

use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

/// egui 가 지연에서 미리 빼는 예상 프레임 시간. egui 는 vsync 루프를 가정해 목표 시각보다 한 프레임
/// 일찍 깨우고, 그 프레임에서 경계에 닿지 않았으면 즉시 repaint 를 한 번 더 요청한다. 타이머는
/// 정시에 깨우므로 이 값을 되돌려 경계마다 한 프레임만 그린다. egui-winit 은 `predicted_dt` 를 바꾸지
/// 않으므로 egui 의 기본값과 같다.
pub(crate) fn egui_frame_allowance() -> Duration {
    Duration::try_from_secs_f32(egui::RawInput::default().predicted_dt).unwrap_or_default()
}

const WHAT: &str = "egui delayed repaint gate";
static POISON_REPORTED: AtomicBool = AtomicBool::new(false);

/// 창 하나의 지연 repaint 예약 상태. repaint 콜백(`Send + Sync`)이 소유한다.
#[derive(Default)]
pub(crate) struct DelayedRepaintGate {
    pending: Mutex<Option<Instant>>,
}

impl DelayedRepaintGate {
    /// `now`에서 `delay` 뒤의 예약을 루프로 보내야 하면 그 시각을 돌려준다.
    /// `delay`가 0이면 즉시 요청이므로 이 관문을 거치지 않는다. 시각을 셀 수 없을 만큼 긴 지연은 버린다.
    pub(crate) fn admit(&self, delay: Duration, now: Instant) -> Option<Instant> {
        let at = now.checked_add(delay)?;
        // 잠금을 쥔 채 패닉한 콜백이 남긴 값도 시각 하나뿐이라 그대로 이어 쓴다.
        let mut pending =
            tasty_utils::poison::recover_mutex(self.pending.lock(), WHAT, &POISON_REPORTED);
        let earlier_ahead = pending.is_some_and(|p| p > now && p <= at);
        if earlier_ahead {
            return None;
        }
        *pending = Some(at);
        Some(at)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: Duration = Duration::from_millis(1);

    #[test]
    fn the_first_request_is_admitted_at_its_deadline() {
        let gate = DelayedRepaintGate::default();
        let t0 = Instant::now();
        assert_eq!(gate.admit(500 * MS, t0), Some(t0 + 500 * MS));
    }

    #[test]
    fn a_later_request_waits_behind_an_earlier_pending_one() {
        let gate = DelayedRepaintGate::default();
        let t0 = Instant::now();
        assert!(gate.admit(100 * MS, t0).is_some());
        assert_eq!(gate.admit(500 * MS, t0), None);
        // 같은 시각을 다시 요청해도 다시 보내지 않는다.
        assert_eq!(gate.admit(100 * MS, t0), None);
    }

    #[test]
    fn an_earlier_request_replaces_a_later_pending_one() {
        let gate = DelayedRepaintGate::default();
        let t0 = Instant::now();
        assert!(gate.admit(500 * MS, t0).is_some());
        assert_eq!(gate.admit(100 * MS, t0), Some(t0 + 100 * MS));
    }

    #[test]
    fn a_passed_deadline_lets_the_next_request_through() {
        let gate = DelayedRepaintGate::default();
        let t0 = Instant::now();
        assert!(gate.admit(100 * MS, t0).is_some());
        let later = t0 + 100 * MS;
        assert_eq!(gate.admit(500 * MS, later), Some(later + 500 * MS));
    }

    #[test]
    fn the_frame_allowance_is_eguis_default_frame_time() {
        let allowance = egui_frame_allowance();
        assert!(allowance > Duration::ZERO && allowance < Duration::from_millis(100));
    }

    /// 깜박이는 커서는 포커스된 입력란이 있는 동안 지연 repaint 를 끝없이 낸다.
    #[test]
    fn the_host_theme_keeps_the_text_cursor_steady() {
        let ctx = egui::Context::default();
        tasty_egui_theme::apply_theme_to_egui(&crate::theme::theme(), &ctx);
        assert!(!ctx.style().visuals.text_cursor.blink);
    }

    /// 입력란 커서는 테두리 굵기(1px)의 text-primary 막대다. egui 기본값은 2px 하늘색이다.
    #[test]
    fn the_host_text_cursor_is_a_text_primary_border_width_bar() {
        let th = crate::theme::theme();
        let ctx = egui::Context::default();
        tasty_egui_theme::apply_theme_to_egui(&th, &ctx);
        let stroke = ctx.style().visuals.text_cursor.stroke;
        assert_eq!(stroke.width, th.border_width.value());
        assert_eq!(stroke.color, egui::Color32::from(th.text_primary()));
    }

    #[test]
    fn an_unrepresentable_delay_is_dropped() {
        let gate = DelayedRepaintGate::default();
        assert_eq!(gate.admit(Duration::MAX, Instant::now()), None);
    }
}
