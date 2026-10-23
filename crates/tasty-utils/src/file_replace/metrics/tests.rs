use super::*;
use crate::file_replace::{self, Options};

#[test]
fn actual_replace_delete_success_failure_bytes_and_scope_exclusion_are_distinct() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("one");
    file_replace::replace(&path, b"boot outside writer").unwrap();
    let metrics = Arc::new(FileOperationMetrics::default());
    {
        let _scope = observe_operations(metrics.clone());
        file_replace::replace(&path, b"one").unwrap();
        file_replace::replace_preserving(&root.path().join("two"), b"four", &path).unwrap();
        let blocked = root.path().join("blocked");
        std::fs::create_dir(&blocked).unwrap();
        assert!(file_replace::replace(&blocked, b"discarded").is_err());
        assert!(blocked.is_dir());
        file_replace::remove(&path).unwrap();
        assert_eq!(
            file_replace::remove(&path).unwrap_err().kind(),
            std::io::ErrorKind::NotFound
        );
    }
    let before = metrics.snapshot();
    file_replace::replace(&path, b"CLI outside writer").unwrap();
    file_replace::remove(&path).unwrap();
    assert_eq!(metrics.snapshot(), before);
    assert_eq!(
        (
            before.replace.attempts,
            before.replace.succeeded,
            before.replace.failed,
            before.replace.unknown
        ),
        (3, 2, 1, 0)
    );
    assert_eq!(before.replace.payload_bytes, 16);
    assert_eq!(before.replace.published_bytes, 7);
    assert_eq!(before.replace.durations.iter().sum::<u64>(), 3);
    assert_eq!(before.replace.errors.iter().sum::<u64>(), 1);
    assert_eq!(
        (
            before.delete.attempts,
            before.delete.succeeded,
            before.delete.failed,
            before.delete.unknown
        ),
        (2, 1, 1, 0)
    );
    assert_eq!(before.delete.errors[0], 1);
    assert_eq!(before.delete.durations.iter().sum::<u64>(), 2);
}
#[test]
fn nested_scopes_restore_and_do_not_propagate_to_other_threads() {
    let root = tempfile::tempdir().unwrap();
    let first = Arc::new(FileOperationMetrics::default());
    let second = Arc::new(FileOperationMetrics::default());
    {
        let _outer = observe_operations(first.clone());
        file_replace::replace(&root.path().join("first"), b"a").unwrap();
        {
            let _inner = observe_operations(second.clone());
            file_replace::replace(&root.path().join("second"), b"bb").unwrap();
        }
        let path = root.path().join("outside-thread");
        std::thread::spawn(move || file_replace::replace(&path, b"excluded").unwrap())
            .join()
            .unwrap();
        file_replace::replace(&root.path().join("third"), b"ccc").unwrap();
    }
    assert_eq!(first.snapshot().replace.attempts, 2);
    assert_eq!(first.snapshot().replace.published_bytes, 4);
    assert_eq!(second.snapshot().replace.attempts, 1);
    assert_eq!(second.snapshot().replace.published_bytes, 2);
}
#[test]
fn unwinding_inside_real_temp_write_records_unknown_and_restores_scope() {
    use std::io::Write;
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("target");
    file_replace::replace(&path, b"original").unwrap();
    let metrics = Arc::new(FileOperationMetrics::default());
    let observed = metrics.clone();
    let panic = std::panic::catch_unwind(move || {
        let _scope = observe_operations(observed);
        // The private callback injects a panic after an actual private-temp payload write,
        // at the same measured boundary used by public replace_with.
        measure(Kind::Replace, 3, || {
            file_replace::replace_using(&path, Options::default(), |file| {
                file.write_all(b"new")?;
                panic!("injected during replacement");
            })
        })
        .unwrap();
    });
    assert!(panic.is_err());
    let snapshot = metrics.snapshot().replace;
    assert_eq!(
        (
            snapshot.attempts,
            snapshot.succeeded,
            snapshot.failed,
            snapshot.unknown
        ),
        (1, 0, 0, 1)
    );
    assert_eq!(snapshot.payload_bytes, 3);
    assert_eq!(snapshot.published_bytes, 0);
    assert_eq!(snapshot.durations.iter().sum::<u64>(), 1);
    assert_eq!(
        std::fs::read(root.path().join("target")).unwrap(),
        b"original"
    );
    file_replace::replace(&root.path().join("after"), b"not observed").unwrap();
    assert_eq!(metrics.snapshot().replace, snapshot);
}

#[test]
fn custom_replacement_guard_counts_real_publish_and_io_error_without_double_wrapping() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("custom");
    let outside = observe_replacement(100);
    std::fs::write(&path, b"outside").unwrap();
    outside.finish(Ok(()));
    let metrics = Arc::new(FileOperationMetrics::default());
    let _scope = observe_operations(metrics.clone());
    let attempt = observe_replacement(3);
    let temp = root.path().join("private-temp");
    std::fs::write(&temp, b"new").unwrap();
    let result = std::fs::rename(&temp, &path);
    attempt.finish(result.as_ref().map(|()| ()).map_err(std::io::Error::kind));
    result.unwrap();
    let failed = observe_replacement(5);
    let result = std::fs::rename(root.path().join("missing"), &path);
    assert!(result.is_err());
    failed.finish(result.map_err(|error| error.kind()));
    let snapshot = metrics.snapshot().replace;
    assert_eq!(
        (
            snapshot.attempts,
            snapshot.succeeded,
            snapshot.failed,
            snapshot.unknown
        ),
        (2, 1, 1, 0)
    );
    assert_eq!((snapshot.payload_bytes, snapshot.published_bytes), (8, 3));
    assert_eq!(snapshot.errors[0], 1);
    assert_eq!(snapshot.durations.iter().sum::<u64>(), 2);
    assert_eq!(std::fs::read(path).unwrap(), b"new");
}
#[test]
fn custom_replacement_unfinished_guard_is_unknown_and_thread_scope_restores() {
    let root = tempfile::tempdir().unwrap();
    let metrics = Arc::new(FileOperationMetrics::default());
    let observed = metrics.clone();
    let path = root.path().join("private-temp");
    let panic = std::panic::catch_unwind(move || {
        let _scope = observe_operations(observed);
        let _attempt = observe_replacement(3);
        std::fs::write(path, b"new").unwrap();
        panic!("injected before custom publication");
    });
    assert!(panic.is_err());
    let snapshot = metrics.snapshot().replace;
    assert_eq!(
        (
            snapshot.attempts,
            snapshot.succeeded,
            snapshot.failed,
            snapshot.unknown
        ),
        (1, 0, 0, 1)
    );
    assert_eq!((snapshot.payload_bytes, snapshot.published_bytes), (3, 0));
    assert_eq!(snapshot.durations.iter().sum::<u64>(), 1);
    let outside = observe_replacement(4);
    std::fs::write(root.path().join("outside"), b"four").unwrap();
    outside.finish(Ok(()));
    assert_eq!(metrics.snapshot().replace, snapshot);
}
