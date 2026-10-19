//! One outstanding receipt and one coalesced successor per connection.
//! Times belong to input admission, not checkpoint transmission or PTY output.
use std::time::Instant;

#[derive(Default)]
pub(crate) struct InputReceipts {
    pub(crate) enabled: bool,
    sequence: u64,
    current: Option<(u64, Instant)>,
    successor: Option<Instant>,
}
impl InputReceipts {
    pub(crate) fn input(&mut self, admitted: Instant) -> Option<u64> {
        if !self.enabled {
            return None;
        }
        if self.current.is_some() {
            self.successor.get_or_insert(admitted);
            return None;
        }
        self.start(admitted)
    }
    fn start(&mut self, admitted: Instant) -> Option<u64> {
        self.sequence = self.sequence.checked_add(1)?;
        self.current = Some((self.sequence, admitted));
        Some(self.sequence)
    }
    pub(crate) fn received(&mut self, sequence: u64) -> Option<u64> {
        if self
            .current
            .is_none_or(|(expected, _)| expected != sequence)
        {
            return None;
        }
        self.current = None;
        self.successor
            .take()
            .and_then(|admitted| self.start(admitted))
    }
    pub(crate) fn oldest(&self) -> Option<Instant> {
        self.current.map(|(_, admitted)| admitted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn successor_keeps_first_input_time_and_ignores_unrelated_receipts() {
        let t = Instant::now();
        let mut state = InputReceipts {
            enabled: true,
            ..Default::default()
        };
        assert_eq!(state.input(t), Some(1));
        assert_eq!(state.input(t + Duration::from_secs(2)), None);
        assert_eq!(state.input(t + Duration::from_secs(3)), None);
        assert_eq!(state.received(999), None);
        assert_eq!(state.oldest(), Some(t));
        assert_eq!(state.received(1), Some(2));
        assert_eq!(state.oldest(), Some(t + Duration::from_secs(2)));
        assert_eq!(state.received(1), None);
        assert_eq!(
            (t + Duration::from_secs(5))
                .duration_since(state.oldest().unwrap())
                .as_secs(),
            3
        );
        assert_eq!(state.received(2), None);
        assert_eq!(state.oldest(), None);
    }

    #[test]
    fn old_server_never_acquires_an_input_wait() {
        let mut state = InputReceipts::default();
        assert_eq!(state.input(Instant::now()), None);
        assert_eq!(state.received(1), None);
        assert_eq!(state.oldest(), None);
    }
}
