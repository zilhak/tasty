//! Standalone policy on the existing resource collection; no second resource registry.
use super::TerminalStore;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use tasty_terminal::Pty;

pub(crate) const DEFAULT_MAX_CONCURRENT: usize = 8;
pub(crate) const DEFAULT_IDLE_TTL: Duration = Duration::from_secs(300);
pub(crate) const PTY_ID_BASE: u32 = 0x8000_0000;
pub(crate) const fn is_surface_id_space(id: u32) -> bool {
    id < PTY_ID_BASE
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PtySpawnError {
    LimitReached { current: usize, max: usize },
    IdExhausted,
}
impl std::fmt::Display for PtySpawnError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LimitReached { current, max } => write!(
                f,
                "headless PTY concurrency limit reached ({current}/{max})"
            ),
            Self::IdExhausted => f.write_str("headless PTY ID space exhausted"),
        }
    }
}
impl std::error::Error for PtySpawnError {}

impl TerminalStore {
    pub(crate) fn reserve_pty_id(&self) -> Result<u32, PtySpawnError> {
        let current = self.standalone_iter().count();
        if current >= self.max_standalone {
            return Err(PtySpawnError::LimitReached {
                current,
                max: self.max_standalone,
            });
        }
        self.next_pty_id
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .map_err(|_| PtySpawnError::IdExhausted)
    }
    pub(crate) fn is_standalone(&self, id: u32) -> bool {
        self.standalone(id).is_some()
    }
    pub(crate) fn standalone(&self, id: u32) -> Option<&Pty> {
        self.pty(id)
            .filter(|pty| pty.state().standalone().is_some())
    }
    pub(crate) fn standalone_iter(&self) -> impl Iterator<Item = (u32, &Pty)> {
        self.terminals.iter().filter_map(|(&id, (_, pty))| {
            pty.as_ref()
                .filter(|pty| pty.state().standalone().is_some())
                .map(|pty| (id, pty))
        })
    }
    pub(crate) fn touch_standalone(&mut self, id: u32, now: Instant) {
        if let Some(pty) = self.pty_mut(id) {
            pty.touch(now);
        }
    }
    pub(crate) fn expired_standalone_ids(&self, now: Instant) -> Vec<u32> {
        self.standalone_iter()
            .filter(|(_, pty)| {
                now.saturating_duration_since(
                    pty.state().standalone().expect("standalone").last_activity,
                ) >= self.idle_ttl
            })
            .map(|(id, _)| id)
            .collect()
    }
    #[cfg(test)]
    pub(crate) fn set_standalone_limits(&mut self, max: usize, ttl: Duration) {
        self.max_standalone = max;
        self.idle_ttl = ttl;
    }
}
