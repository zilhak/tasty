use super::*;

fn full_disk_after(shared: &Shared, bytes: u64) -> String {
    let code = if cfg!(windows) { 112 } else { 28 };
    *lock(&shared.write_failure_after) = Some((bytes, code));
    let error = io::Error::from_raw_os_error(code);
    assert_eq!(error.kind(), io::ErrorKind::StorageFull);
    error.to_string()
}

fn setup() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let root = tempfile::tempdir().unwrap();
    let src = root.path().join("src");
    let dst = root.path().join("dst");
    std::fs::create_dir(&src).unwrap();
    std::fs::create_dir(&dst).unwrap();
    (root, src, dst)
}

#[test]
fn disk_full_mid_copy_preserves_source_and_removes_partial_staging() {
    let (_root, src, dst) = setup();
    let path = src.join("large.bin");
    let bytes = vec![7; COPY_CHUNK * 3];
    std::fs::write(&path, &bytes).unwrap();
    let shared = Shared::fixed(Choice::KeepBoth);
    let message = full_disk_after(&shared, COPY_CHUNK as u64);
    let report = run_transfer(&shared, std::slice::from_ref(&path), &dst, false);
    assert_eq!(report.done, 0);
    assert_eq!(report.failed.len(), 1);
    assert!(matches!(&report.failed[0].reason, Reason::Os(s) if s == &message));
    assert_eq!(report.retryable(), vec![path.clone()]);
    assert!(report.undo.is_empty());
    assert!(report.staging_left.is_empty());
    assert!(!report.cancelled);
    assert_eq!(std::fs::read_dir(&dst).unwrap().count(), 0);
    assert_eq!(std::fs::read(path).unwrap(), bytes);
}

#[test]
fn disk_full_before_replace_keeps_the_existing_destination() {
    let (_root, src, dst) = setup();
    let path = src.join("a.txt");
    std::fs::write(&path, b"incoming").unwrap();
    std::fs::write(dst.join("a.txt"), b"existing").unwrap();
    let shared = Shared::fixed(Choice::Replace);
    full_disk_after(&shared, 0);
    let report = run_transfer(&shared, std::slice::from_ref(&path), &dst, false);
    assert_eq!(report.done, 0);
    assert_eq!(report.failed.len(), 1);
    assert!(report.undo.is_empty());
    assert!(report.staging_left.is_empty());
    assert_eq!(std::fs::read(dst.join("a.txt")).unwrap(), b"existing");
    assert_eq!(std::fs::read(path).unwrap(), b"incoming");
    assert_eq!(std::fs::read_dir(dst).unwrap().count(), 1);
}

#[test]
fn disk_full_after_one_item_only_retries_the_failed_item() {
    let (_root, src, dst) = setup();
    let first = src.join("a.txt");
    let second = src.join("b.bin");
    std::fs::write(&first, b"first").unwrap();
    std::fs::write(&second, vec![7; COPY_CHUNK * 2]).unwrap();
    let shared = Shared::fixed(Choice::KeepBoth);
    full_disk_after(&shared, 5);
    let report = run_transfer(&shared, &[first.clone(), second.clone()], &dst, false);
    assert_eq!(report.done, 1);
    assert_eq!(report.failed.len(), 1);
    assert_eq!(report.retryable(), vec![second.clone()]);
    assert_eq!(report.undo.len(), 1);
    assert!(report.staging_left.is_empty());
    assert_eq!(std::fs::read(dst.join("a.txt")).unwrap(), b"first");
    assert!(!dst.join("b.bin").exists());
    assert!(first.exists() && second.exists());
    assert_eq!(std::fs::read_dir(dst).unwrap().count(), 1);
}
