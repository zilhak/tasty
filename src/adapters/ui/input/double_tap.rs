use std::time::Instant;
use winit::keyboard::{Key, NamedKey};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DoubleTapKey {
    Shift,
    Ctrl,
    Alt,
}

impl DoubleTapKey {
    pub fn binding_str(&self) -> &'static str {
        match self {
            DoubleTapKey::Shift => "shift+shift",
            DoubleTapKey::Ctrl => "ctrl+ctrl",
            DoubleTapKey::Alt => "alt+alt",
        }
    }
}

/// 수식키 단독 누름·떼기 뒤 제한 시간 안에 같은 키를 다시 누르면 double tap으로 처리한다.
/// 누르는 동안 다른 키가 들어오면 해당 탭은 취소한다.
pub struct DoubleTapDetector {
    /// Maximum time between two taps (ms).
    threshold_ms: u128,
    /// The modifier currently being pressed (waiting for release).
    pending_key: Option<DoubleTapKey>,
    /// Whether another key was pressed during the current modifier hold.
    contaminated: bool,
    /// First tap: which key and when it was released.
    first_tap: Option<(DoubleTapKey, Instant)>,
    /// Fired double-tap event, consumed by the next poll.
    fired: Option<DoubleTapKey>,
}

impl DoubleTapDetector {
    pub fn new() -> Self {
        Self {
            threshold_ms: 400,
            pending_key: None,
            contaminated: false,
            first_tap: None,
            fired: None,
        }
    }

    pub fn on_key_event(&mut self, key: &Key, pressed: bool) {
        let modifier = Self::as_modifier(key);

        if pressed {
            if let Some(m) = modifier {
                if self.pending_key.is_none() {
                    self.pending_key = Some(m);
                    self.contaminated = false;

                    if let Some((first_key, first_time)) = &self.first_tap
                        && *first_key == m
                        && first_time.elapsed().as_millis() < self.threshold_ms
                    {
                        self.fired = Some(m);
                        self.first_tap = None;
                        self.pending_key = None;
                    }
                }
            } else {
                self.contaminated = true;
                self.first_tap = None;
            }
        } else {
            if let Some(m) = modifier
                && self.pending_key == Some(m)
            {
                if !self.contaminated {
                    self.first_tap = Some((m, Instant::now()));
                }
                self.pending_key = None;
                self.contaminated = false;
            }
        }
    }

    pub fn take(&mut self) -> Option<DoubleTapKey> {
        self.fired.take()
    }

    /// 포커스 획득·상실 때 이전 탭을 버린다. 창 밖의 release는 합성 이벤트로 걸러질 수 있어
    /// 이전 press와 돌아온 뒤 입력을 연결하면 탭 한 번을 두 번으로 오인할 수 있다.
    pub fn reset(&mut self) {
        self.pending_key = None;
        self.contaminated = false;
        self.first_tap = None;
        self.fired = None;
    }

    fn as_modifier(key: &Key) -> Option<DoubleTapKey> {
        match key {
            Key::Named(NamedKey::Shift) => Some(DoubleTapKey::Shift),
            Key::Named(NamedKey::Control) => Some(DoubleTapKey::Ctrl),
            Key::Named(NamedKey::Alt) => Some(DoubleTapKey::Alt),
            // macOS: Cmd key (Super) maps to Alt in our binding system
            #[cfg(target_os = "macos")]
            Key::Named(NamedKey::Super) => Some(DoubleTapKey::Alt),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alt() -> Key {
        Key::Named(NamedKey::Alt)
    }

    #[test]
    fn two_clean_taps_fire() {
        let mut d = DoubleTapDetector::new();
        d.on_key_event(&alt(), true);
        d.on_key_event(&alt(), false);
        d.on_key_event(&alt(), true);
        assert_eq!(d.take(), Some(DoubleTapKey::Alt));
    }

    #[test]
    fn focus_change_reset_prevents_stale_first_tap() {
        let mut d = DoubleTapDetector::new();
        d.on_key_event(&alt(), true);
        d.reset();

        d.on_key_event(&alt(), false);
        d.on_key_event(&alt(), true);
        assert_eq!(d.take(), None);
    }

    #[test]
    fn reset_discards_recorded_first_tap() {
        let mut d = DoubleTapDetector::new();
        d.on_key_event(&alt(), true);
        d.on_key_event(&alt(), false);
        d.reset();
        d.on_key_event(&alt(), true);
        assert_eq!(d.take(), None);
    }
}
