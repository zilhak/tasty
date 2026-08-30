use std::time::Instant;
use winit::keyboard::{Key, NamedKey};

/// Which modifier key was double-tapped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DoubleTapKey {
    Shift,
    Ctrl,
    Alt,
}

impl DoubleTapKey {
    /// The binding string for this double-tap (e.g. "shift+shift").
    pub fn binding_str(&self) -> &'static str {
        match self {
            DoubleTapKey::Shift => "shift+shift",
            DoubleTapKey::Ctrl => "ctrl+ctrl",
            DoubleTapKey::Alt => "alt+alt",
        }
    }
}

/// Detect double-tap of modifier keys (Shift, Ctrl, Alt).
///
/// Detection logic:
/// 1. Modifier key pressed alone → record
/// 2. Any other key pressed while modifier is held → invalidate
/// 3. Modifier key released (clean, no other key) → record as "first tap" with timestamp
/// 4. Same modifier pressed again within threshold → fire double-tap!
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

    /// Call on every KeyboardInput event (both Press and Release).
    pub fn on_key_event(&mut self, key: &Key, pressed: bool) {
        let modifier = Self::as_modifier(key);

        if pressed {
            if let Some(m) = modifier {
                // Modifier pressed
                if self.pending_key.is_none() {
                    self.pending_key = Some(m);
                    self.contaminated = false;

                    // Check if this is the second tap
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
                // Non-modifier key pressed → contaminate
                self.contaminated = true;
                self.first_tap = None;
            }
        } else {
            // Key released
            if let Some(m) = modifier
                && self.pending_key == Some(m)
            {
                if !self.contaminated {
                    // Clean release → record as first tap
                    self.first_tap = Some((m, Instant::now()));
                }
                self.pending_key = None;
                self.contaminated = false;
            }
        }
    }

    /// Take the fired double-tap event (if any). Returns None if no double-tap occurred.
    pub fn take(&mut self) -> Option<DoubleTapKey> {
        self.fired.take()
    }

    /// 진행 중인 탭 추적을 전부 버린다. 창의 포커스가 바뀌는 시점에 호출한다.
    ///
    /// 포커스 경계를 넘으면 modifier 의 down/up 짝이 이 창 안에서 완결되지 않는다.
    /// `Alt+Tab` 으로 빠져나가면 `Alt` 의 press 만 여기로 들어오고, 짝이 되는 release 는
    /// winit 의 합성 이벤트로 오는데 그건 사용자 입력이 아니라 버려진다
    /// (`super::synthetic`). 그대로 두면 `pending_key` 가 남아, 돌아와서 `Alt` 를 떼는
    /// 순간 "clean release" 로 오인돼 first tap 이 기록되고 다음 실제 탭 한 번에
    /// double-tap 이 오발화한다. 양방향(획득/상실) 모두에서 지운다 — 어느 쪽이든
    /// 이전 상태를 신뢰할 수 없다.
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

    /// 기준선 — 같은 modifier 를 두 번 탭하면 발화한다. 아래 reset 테스트가
    /// "아무것도 발화하지 않는 detector" 로 통과하지 않게 잡아주는 대조군이다.
    #[test]
    fn two_clean_taps_fire() {
        let mut d = DoubleTapDetector::new();
        d.on_key_event(&alt(), true);
        d.on_key_event(&alt(), false);
        d.on_key_event(&alt(), true);
        assert_eq!(d.take(), Some(DoubleTapKey::Alt));
    }

    /// `Alt` 를 누른 채 창을 벗어났다 돌아온 경우. 합성 release 는 버려지므로
    /// `pending_key` 가 남고, 포커스 시점에 지우지 않으면 돌아와서 `Alt` 를 떼는 것이
    /// first tap 으로 기록돼 다음 탭 한 번에 double-tap 이 오발화한다.
    #[test]
    fn focus_change_reset_prevents_stale_first_tap() {
        let mut d = DoubleTapDetector::new();
        // 창 안에서 Alt 를 누른 상태로 포커스가 떠난다 (짝이 되는 release 는 합성이라
        // 이 detector 에 오지 않는다).
        d.on_key_event(&alt(), true);
        d.reset();

        // 포커스 복귀 후 사용자가 Alt 를 뗀다 — 눌린 적을 본 일이 없으므로 first tap 이
        // 기록되면 안 된다.
        d.on_key_event(&alt(), false);
        // 이어지는 실제 탭 한 번으로 double-tap 이 발화하면 안 된다.
        d.on_key_event(&alt(), true);
        assert_eq!(d.take(), None);
    }

    /// reset 은 이미 기록된 first tap 도 버린다 — 포커스가 오간 뒤의 탭 한 번은
    /// 이전 탭과 짝지어지지 않는다.
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
