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
                    if let Some((first_key, first_time)) = &self.first_tap {
                        if *first_key == m && first_time.elapsed().as_millis() < self.threshold_ms {
                            self.fired = Some(m);
                            self.first_tap = None;
                            self.pending_key = None;
                            return;
                        }
                    }
                }
            } else {
                // Non-modifier key pressed → contaminate
                self.contaminated = true;
                self.first_tap = None;
            }
        } else {
            // Key released
            if let Some(m) = modifier {
                if self.pending_key == Some(m) {
                    if !self.contaminated {
                        // Clean release → record as first tap
                        self.first_tap = Some((m, Instant::now()));
                    }
                    self.pending_key = None;
                    self.contaminated = false;
                }
            }
        }
    }

    /// Take the fired double-tap event (if any). Returns None if no double-tap occurred.
    pub fn take(&mut self) -> Option<DoubleTapKey> {
        self.fired.take()
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
