//! The Explorer App shutdown deadline, separate from worker join observation.
use std::time::Instant;

pub(super) fn keep_waiting(remaining: usize, deadline: &mut Option<Instant>, now: Instant) -> bool {
    if remaining != 0
        && let Some(until) = *deadline
    {
        if now < until {
            return true;
        }
        tracing::warn!(
            remaining,
            "View I/O shutdown timed out; worker remains unjoined and accepted file work is not cancelled"
        );
        *deadline = None;
    }
    false
}

#[cfg(test)]
#[path = "explorer_shutdown_wait_tests.rs"]
mod tests;
