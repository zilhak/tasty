//! Pre-transfer traversal runs on the file-job worker and never follows links.

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use super::Shared;

const COUNT_UPDATE_INTERVAL_MS: u64 = 250;

pub(super) struct Measure {
    pub bytes: Option<u64>,
    pub files: usize,
    pub first_file: Option<PathBuf>,
}

fn update_due(last: u64, now: u64) -> bool {
    now.saturating_sub(last) >= COUNT_UPDATE_INTERVAL_MS
}

pub(super) fn measure(shared: &Shared, root: &Path, found: &mut usize) -> Option<Measure> {
    let mut stack = vec![root.to_path_buf()];
    let mut result = Measure {
        bytes: Some(0),
        files: 0,
        first_file: None,
    };
    while let Some(path) = stack.pop() {
        if shared.cancelled() {
            return None;
        }
        let meta = match path.symlink_metadata() {
            Ok(meta) => meta,
            Err(_) => {
                result.bytes = None;
                continue;
            }
        };
        if meta.is_dir() {
            match std::fs::read_dir(&path) {
                Ok(entries) => {
                    for entry in entries {
                        if shared.cancelled() {
                            return None;
                        }
                        match entry {
                            Ok(entry) => stack.push(entry.path()),
                            Err(_) => result.bytes = None,
                        }
                    }
                }
                Err(_) => result.bytes = None,
            }
        } else {
            result.files = result.files.saturating_add(1);
            *found = found.saturating_add(1);
            if result.first_file.is_none() {
                result.first_file = Some(path);
            }
            let bytes = if meta.file_type().is_symlink() {
                Some(0)
            } else if meta.is_file() {
                Some(meta.len())
            } else {
                None
            };
            result.bytes = result.bytes.and_then(|sum| sum.checked_add(bytes?));
        }
        if update_due(
            shared.last_wake_ms.load(Ordering::Acquire),
            shared.elapsed_ms(),
        ) {
            shared.files_total.store(*found, Ordering::Release);
            shared.wake_now();
        }
    }
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::file_jobs::job::Choice;

    #[test]
    fn counting_publishes_at_most_four_updates_per_second() {
        let mut last = 0;
        let mut updates = Vec::new();
        for now in 0..=1000 {
            if update_due(last, now) {
                updates.push(now);
                last = now;
            }
        }
        assert_eq!(updates, [250, 500, 750, 1000]);
    }

    #[test]
    fn unreadable_totals_are_unknown_and_cancelled_traversal_stops() {
        let shared = Shared::fixed(Choice::KeepBoth);
        let dir = tempfile::tempdir().unwrap();
        let measured = measure(&shared, &dir.path().join("absent"), &mut 0).unwrap();
        assert_eq!(measured.bytes, None);
        assert_eq!(measured.files, 0);
        shared.cancel();
        assert!(measure(&shared, dir.path(), &mut 0).is_none());
    }
}
