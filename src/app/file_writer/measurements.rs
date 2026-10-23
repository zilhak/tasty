//! Fixed aggregate metadata for executed ordered commands; physical operations are separate.
use super::command::{CommandDisposition, CommandOutcome};
use std::sync::atomic::{AtomicU64, Ordering};
#[derive(Debug, Default)]
pub(super) struct Commands {
    known_success: AtomicU64,
    known_failure: AtomicU64,
    unknown: AtomicU64,
    unclassified: AtomicU64,
    partial: AtomicU64,
}
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CommandCounts {
    pub(crate) known_success: u64,
    pub(crate) known_failure: u64,
    pub(crate) unknown: u64,
    pub(crate) unclassified: u64,
    pub(crate) partial: u64,
}
impl Commands {
    pub(super) fn record(&self, disposition: Option<CommandDisposition>) {
        let counter = match disposition {
            None => &self.unclassified,
            Some(value) => {
                if value.partial {
                    self.partial.fetch_add(1, Ordering::Relaxed);
                }
                match value.outcome {
                    CommandOutcome::KnownSuccess => &self.known_success,
                    CommandOutcome::KnownFailure => &self.known_failure,
                    CommandOutcome::Unknown => &self.unknown,
                }
            }
        };
        counter.fetch_add(1, Ordering::Relaxed);
    }
    pub(super) fn snapshot(&self) -> CommandCounts {
        CommandCounts {
            known_success: self.known_success.load(Ordering::Relaxed),
            known_failure: self.known_failure.load(Ordering::Relaxed),
            unknown: self.unknown.load(Ordering::Relaxed),
            unclassified: self.unclassified.load(Ordering::Relaxed),
            partial: self.partial.load(Ordering::Relaxed),
        }
    }
}
