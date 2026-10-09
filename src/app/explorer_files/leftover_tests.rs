//! 남은 원본 다시 지우기 시험. 원본 삭제 실패는 쓰기 권한을 뺀 하위 폴더로 만든다.

#[cfg(unix)]
use super::super::tests::with_item;
use super::super::{COPY_CHUNK, Choice};
#[cfg(unix)]
use super::super::{Publish, record};
use super::*;

fn shared() -> Shared {
    Shared::fixed(Choice::KeepBoth)
}

fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .expect("list a test folder")
        .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// 원본 `src/payload` 의 하위 폴더 ro 를 지울 수 없게 해 다른 디스크 이동을 흉내 낸다.
/// 원본이 남은 보고를 갖고, 끝나면 ro 의 권한을 되돌린다.
#[cfg(unix)]
struct Stuck {
    _dir: tempfile::TempDir,
    payload: PathBuf,
    copy: PathBuf,
    report: Report,
}

#[cfg(unix)]
impl Stuck {
    fn new() -> Option<Self> {
        // root 는 권한을 무시해 원본이 다 지워진다. 이 경우 재현할 수 없다.
        // SAFETY: geteuid 는 인자 없이 현재 프로세스의 유효 사용자 ID 만 읽는다.
        if unsafe { libc::geteuid() } == 0 {
            return None;
        }
        let dir = tempfile::tempdir().expect("tempdir");
        let payload = dir.path().join("src").join("payload");
        let dst = dir.path().join("dst");
        std::fs::create_dir_all(payload.join("ro")).expect("mkdir");
        std::fs::create_dir_all(&dst).expect("mkdir");
        std::fs::write(payload.join("a.txt"), "a").expect("write");
        std::fs::write(payload.join("ro").join("b.txt"), "b").expect("write");
        let stuck = Self {
            payload: payload.clone(),
            copy: dst.join("payload"),
            report: Report::new(OpKind::Move, Some(dst.clone()), 1),
            _dir: dir,
        };
        stuck.lock(true);
        let shared = shared();
        let meta = payload.symlink_metadata().expect("meta");
        let result = with_item(&shared, &dst, true, |item| {
            item.copy(&payload, &meta, dst.join("payload"), Publish::NoReplace)
        });
        let mut stuck = stuck;
        assert!(record(&mut stuck.report, &shared, &payload, result).is_continue());
        Some(stuck)
    }
    fn lock(&self, locked: bool) {
        use std::os::unix::fs::PermissionsExt;
        let mode = if locked { 0o555 } else { 0o755 };
        std::fs::set_permissions(
            self.payload.join("ro"),
            std::fs::Permissions::from_mode(mode),
        )
        .expect("chmod");
    }
}

#[cfg(unix)]
impl Drop for Stuck {
    fn drop(&mut self) {
        if self.payload.join("ro").exists() {
            self.lock(false);
        }
    }
}

/// 원본이 남은 이동은 사본 경로와 함께 다시 지울 항목으로 남는다.
#[cfg(unix)]
#[test]
fn a_move_that_left_its_original_records_the_copy_for_retry() {
    let Some(stuck) = Stuck::new() else { return };
    assert!(stuck.report.retryable().is_empty());
    assert_eq!(stuck.report.leftovers.len(), 1);
    let left = &stuck.report.leftovers[0];
    assert_eq!(left.source, stuck.payload);
    assert_eq!(left.copy, stuck.copy);
    assert_eq!(left.copy_kind, Some(EntryKind::Dir));
}

/// 막힌 원인이 풀리면 Retry 는 남은 원본만 지우고 사본은 그대로 둔다.
#[cfg(unix)]
#[test]
fn retry_removes_the_original_once_it_can() {
    let Some(stuck) = Stuck::new() else { return };
    stuck.lock(false);
    let report = run_remove_leftovers(&shared(), None, &stuck.report.leftovers);
    assert_eq!((report.done, report.total), (1, 1));
    assert!(report.failed.is_empty(), "{:?}", report.failed);
    assert!(report.leftovers.is_empty());
    assert!(report.undo.is_empty());
    assert!(stuck.payload.symlink_metadata().is_err());
    assert_eq!(names(&stuck.copy), ["a.txt", "ro"]);
    assert_eq!(names(&stuck.copy.join("ro")), ["b.txt"]);
}

/// 아직 지울 수 없으면 같은 사유로 남고, 한 번 더 시도할 수 있다.
#[cfg(unix)]
#[test]
fn retry_that_still_cannot_remove_keeps_the_item_for_another_retry() {
    let Some(stuck) = Stuck::new() else { return };
    let report = run_remove_leftovers(&shared(), None, &stuck.report.leftovers);
    assert!(matches!(
        report.failed.as_slice(),
        [Failure {
            reason: Reason::SourceNotRemoved(_),
            ..
        }]
    ));
    assert_eq!(report.leftovers, stuck.report.leftovers);
    assert_eq!(names(&stuck.payload.join("ro")), ["b.txt"]);
}

/// 사본이 없어졌으면 하나 남은 원본을 지우지 않는다.
#[cfg(unix)]
#[test]
fn retry_keeps_the_original_when_the_copy_is_gone() {
    let Some(stuck) = Stuck::new() else { return };
    stuck.lock(false);
    std::fs::remove_dir_all(&stuck.copy).expect("remove the copy");
    let report = run_remove_leftovers(&shared(), None, &stuck.report.leftovers);
    assert_eq!(
        report.failed,
        [Failure {
            path: stuck.payload.clone(),
            reason: Reason::CopyMissing,
        }]
    );
    assert_eq!(report.done, 0);
    assert!(report.leftovers.is_empty());
    assert_eq!(names(&stuck.payload.join("ro")), ["b.txt"]);
}

/// 사본 자리에 다른 종류의 항목이 있으면 사본이 없어진 것으로 본다.
#[cfg(unix)]
#[test]
fn retry_keeps_the_original_when_the_copy_became_another_kind() {
    let Some(stuck) = Stuck::new() else { return };
    stuck.lock(false);
    std::fs::remove_dir_all(&stuck.copy).expect("remove the copy");
    std::fs::write(&stuck.copy, "not the folder").expect("write a file there");
    let report = run_remove_leftovers(&shared(), None, &stuck.report.leftovers);
    assert!(matches!(
        report.failed.as_slice(),
        [Failure {
            reason: Reason::CopyMissing,
            ..
        }]
    ));
    assert_eq!(names(&stuck.payload.join("ro")), ["b.txt"]);
}

/// 원본 파일이 그 뒤 바뀌었으면 사용자가 고친 것으로 보고 남긴다.
#[test]
fn retry_keeps_an_original_file_changed_after_the_move() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = dir.path().join("a.txt");
    let copy = dir.path().join("copy.txt");
    std::fs::write(&source, "old").expect("write");
    std::fs::write(&copy, "old").expect("write");
    let left = Leftover::before_remove(&source, &copy);
    std::fs::write(&source, "edited later").expect("edit the original");
    let report = run_remove_leftovers(&shared(), None, &[left]);
    assert_eq!(
        report.failed,
        [Failure {
            path: source.clone(),
            reason: Reason::ChangedSince,
        }]
    );
    assert_eq!(
        std::fs::read_to_string(&source).expect("read"),
        "edited later"
    );
}

/// 원본이 이미 없으면 지울 것이 없으니 끝난 것으로 본다. 사본은 건드리지 않는다.
#[test]
fn retry_counts_an_original_already_gone_as_done() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = dir.path().join("a.txt");
    let copy = dir.path().join("copy.txt");
    std::fs::write(&source, "x").expect("write");
    std::fs::write(&copy, "x").expect("write");
    let left = Leftover::before_remove(&source, &copy);
    std::fs::remove_file(&source).expect("remove");
    let report = run_remove_leftovers(&shared(), None, &[left]);
    assert_eq!((report.done, report.failed.len()), (1, 0));
    assert_eq!(std::fs::read_to_string(&copy).expect("read"), "x");
}

/// 파일 원본이 그대로면 지운다.
#[test]
fn retry_removes_an_unchanged_original_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = dir.path().join("a.txt");
    let copy = dir.path().join("copy.txt");
    std::fs::write(&source, "x").expect("write");
    std::fs::write(&copy, "x").expect("write");
    let left = Leftover::before_remove(&source, &copy);
    let report = run_remove_leftovers(&shared(), None, &[left]);
    assert_eq!((report.done, report.failed.len()), (1, 0));
    assert!(source.symlink_metadata().is_err());
    assert!(copy.exists());
}

/// 같은 내용의 폴더 둘(원본·사본)을 만든다. 원본 일부가 이미 지워진 이동 뒤의 모습이다.
struct Twin {
    _dir: tempfile::TempDir,
    source: PathBuf,
    copy: PathBuf,
}

impl Twin {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let source = dir.path().join("src").join("payload");
        let copy = dir.path().join("dst").join("payload");
        for root in [&source, &copy] {
            std::fs::create_dir_all(root.join("sub")).expect("mkdir");
            std::fs::write(root.join("a.txt"), "a").expect("write");
            std::fs::write(root.join("sub").join("b.txt"), "b").expect("write");
        }
        Self {
            _dir: dir,
            source,
            copy,
        }
    }
    fn retry(&self) -> Report {
        let left = Leftover::before_remove(&self.source, &self.copy);
        run_remove_leftovers(&shared(), None, &[left])
    }
    fn assert_copy_untouched(&self) {
        assert_eq!(names(&self.copy), ["a.txt", "sub"]);
        assert_eq!(names(&self.copy.join("sub")), ["b.txt"]);
    }
}

/// 사본과 같은 항목만 남은 폴더 원본은 통째로 지운다.
#[test]
fn retry_removes_a_folder_original_that_matches_the_copy() {
    let twin = Twin::new();
    let report = twin.retry();
    assert_eq!((report.done, report.failed.len()), (1, 0));
    assert!(twin.source.symlink_metadata().is_err());
    twin.assert_copy_untouched();
}

/// 이동 뒤 원본 폴더에 넣은 파일은 사본에 없으므로 남기고, 남긴 수를 알리며 다시 시도할 수 있다.
#[test]
fn retry_keeps_a_file_added_to_a_folder_original_after_the_move() {
    let twin = Twin::new();
    std::fs::write(twin.source.join("new.txt"), "added later").expect("write");
    let report = twin.retry();
    assert_eq!(
        report.failed,
        [Failure {
            path: twin.source.clone(),
            reason: Reason::KeptNotInCopy(1),
        }]
    );
    assert_eq!(
        report.done, 1,
        "the copy is whole, so the move counts as done"
    );
    assert_eq!(report.leftovers.len(), 1, "the rest can be retried");
    assert!(
        report.retryable().is_empty(),
        "the folder is not moved again"
    );
    assert_eq!(names(&twin.source), ["new.txt"]);
    twin.assert_copy_untouched();

    // 사용자가 남은 파일을 치우면 다시 시도에서 폴더까지 지운다.
    std::fs::remove_file(twin.source.join("new.txt")).expect("remove");
    let again = run_remove_leftovers(&shared(), None, &report.leftovers);
    assert_eq!((again.done, again.failed.len()), (1, 0));
    assert!(twin.source.symlink_metadata().is_err());
}

/// 크기가 달라진 파일은 이동 뒤 고친 것으로 보고 남긴다.
#[test]
fn retry_keeps_a_file_changed_inside_a_folder_original() {
    let twin = Twin::new();
    std::fs::write(twin.source.join("a.txt"), "edited after the move").expect("edit");
    let report = twin.retry();
    assert!(matches!(
        report.failed.as_slice(),
        [Failure {
            reason: Reason::KeptNotInCopy(1),
            ..
        }]
    ));
    assert_eq!(names(&twin.source), ["a.txt"]);
    assert_eq!(
        std::fs::read_to_string(twin.source.join("a.txt")).expect("read"),
        "edited after the move"
    );
    twin.assert_copy_untouched();
}

/// 하위 폴더 안에 넣은 파일도 남기고, 그 파일을 담은 하위 폴더도 남는다.
#[test]
fn retry_keeps_a_file_added_inside_a_subfolder_of_the_original() {
    let twin = Twin::new();
    std::fs::write(twin.source.join("sub").join("new.txt"), "added").expect("write");
    let report = twin.retry();
    assert!(matches!(
        report.failed.as_slice(),
        [Failure {
            reason: Reason::KeptNotInCopy(1),
            ..
        }]
    ));
    assert_eq!(names(&twin.source), ["sub"]);
    assert_eq!(names(&twin.source.join("sub")), ["new.txt"]);
    twin.assert_copy_untouched();
}

/// 사본에 없는 하위 폴더는 통째로 남기고 하나로 센다.
#[test]
fn retry_keeps_a_subfolder_added_to_the_original_as_one_item() {
    let twin = Twin::new();
    std::fs::create_dir_all(twin.source.join("later").join("deep")).expect("mkdir");
    std::fs::write(twin.source.join("later").join("deep").join("x"), "x").expect("write");
    let report = twin.retry();
    assert!(matches!(
        report.failed.as_slice(),
        [Failure {
            reason: Reason::KeptNotInCopy(1),
            ..
        }]
    ));
    assert_eq!(names(&twin.source), ["later"]);
    assert_eq!(names(&twin.source.join("later").join("deep")), ["x"]);
}

/// 링크는 따라가지 않고 링크 자체로 비교한다. 대상이 같은 링크만 지우고, 링크가 가리키는 것은
/// 건드리지 않는다. 대상이 바뀐 링크와, 사본에서는 파일인 자리의 링크는 남긴다.
#[cfg(unix)]
#[test]
fn retry_compares_links_as_links() {
    use std::os::unix::fs::symlink;
    let twin = Twin::new();
    let outside = twin.source.parent().unwrap().join("outside.txt");
    std::fs::write(&outside, "not part of the move").expect("write");
    for root in [&twin.source, &twin.copy] {
        symlink(&outside, root.join("same")).expect("link");
        symlink("sub", root.join("to-sub")).expect("link");
    }
    symlink("a.txt", twin.source.join("retarget")).expect("link");
    symlink("sub", twin.copy.join("retarget")).expect("link");
    symlink("a.txt", twin.source.join("was-file")).expect("link");
    std::fs::write(twin.copy.join("was-file"), "a").expect("write");
    let report = twin.retry();
    assert!(matches!(
        report.failed.as_slice(),
        [Failure {
            reason: Reason::KeptNotInCopy(2),
            ..
        }]
    ));
    assert_eq!(names(&twin.source), ["retarget", "was-file"]);
    assert_eq!(
        std::fs::read_to_string(&outside).expect("read"),
        "not part of the move"
    );
    assert!(twin.copy.join("same").symlink_metadata().is_ok());
}

/// 원본이 링크 하나면 사본 링크와 대상이 같을 때만 지운다.
#[cfg(unix)]
#[test]
fn retry_keeps_a_link_original_whose_target_changed() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().expect("tempdir");
    let (source, copy) = (dir.path().join("l"), dir.path().join("c"));
    symlink("a", &source).expect("link");
    symlink("a", &copy).expect("link");
    let left = Leftover::before_remove(&source, &copy);
    std::fs::remove_file(&source).expect("unlink");
    symlink("b", &source).expect("relink");
    let report = run_remove_leftovers(&shared(), None, std::slice::from_ref(&left));
    assert!(matches!(
        report.failed.as_slice(),
        [Failure {
            reason: Reason::ChangedSince,
            ..
        }]
    ));
    std::fs::remove_file(&source).expect("unlink");
    symlink("a", &source).expect("relink");
    let report = run_remove_leftovers(&shared(), None, &[left]);
    assert_eq!((report.done, report.failed.len()), (1, 0));
    assert!(source.symlink_metadata().is_err());
    assert!(copy.symlink_metadata().is_ok());
}

/// 크기가 같아도 내용이 다르면 이동 뒤 고친 것으로 보고 남긴다.
#[test]
fn retry_keeps_a_same_size_file_whose_content_changed() {
    let twin = Twin::new();
    std::fs::write(twin.source.join("a.txt"), "z").expect("edit, same size");
    let report = twin.retry();
    assert!(matches!(
        report.failed.as_slice(),
        [Failure {
            reason: Reason::KeptNotInCopy(1),
            ..
        }]
    ));
    assert_eq!(
        std::fs::read_to_string(twin.source.join("a.txt")).expect("read"),
        "z"
    );
    twin.assert_copy_untouched();
}

/// 비교 버퍼 하나보다 큰 파일은 첫 버퍼 뒤의 차이도 찾고, 끝까지 같으면 지운다.
#[test]
fn retry_compares_a_large_file_past_the_first_buffer() {
    let twin = Twin::new();
    let mut body = vec![7u8; COPY_CHUNK * 2 + 100];
    std::fs::write(twin.copy.join("big.bin"), &body).expect("write");
    body[COPY_CHUNK + 10] = 8;
    std::fs::write(twin.source.join("big.bin"), &body).expect("write");
    let report = twin.retry();
    assert!(matches!(
        report.failed.as_slice(),
        [Failure {
            reason: Reason::KeptNotInCopy(1),
            ..
        }]
    ));
    assert_eq!(names(&twin.source), ["big.bin"]);

    body[COPY_CHUNK + 10] = 7;
    std::fs::write(twin.source.join("big.bin"), &body).expect("write");
    let again = run_remove_leftovers(&shared(), None, &report.leftovers);
    assert_eq!((again.done, again.failed.len()), (1, 0));
    assert!(twin.source.symlink_metadata().is_err());
}

/// 파일 원본 하나도 수정 시각·크기가 그대로인데 내용이 다르면 남긴다.
#[test]
fn retry_keeps_an_original_file_whose_content_differs_from_the_copy() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = dir.path().join("a.txt");
    let copy = dir.path().join("copy.txt");
    std::fs::write(&source, "old").expect("write");
    std::fs::write(&copy, "old").expect("write");
    let left = Leftover::before_remove(&source, &copy);
    let when = source
        .symlink_metadata()
        .and_then(|m| m.modified())
        .expect("mtime");
    std::fs::write(&source, "new").expect("same size edit");
    std::fs::File::options()
        .write(true)
        .open(&source)
        .and_then(|f| f.set_modified(when))
        .expect("put the time back");
    let report = run_remove_leftovers(&shared(), None, &[left]);
    assert_eq!(
        report.failed,
        [Failure {
            path: source.clone(),
            reason: Reason::ChangedSince,
        }]
    );
    assert_eq!(std::fs::read_to_string(&source).expect("read"), "new");
}

/// 비교하려고 읽지 못한 파일은 같은지 모르므로 남기고 그 오류를 사유로 둔다. 다시 시도할 수 있다.
#[cfg(unix)]
#[test]
fn retry_keeps_a_file_it_cannot_read_for_the_comparison() {
    use std::os::unix::fs::PermissionsExt;
    // SAFETY: geteuid 는 인자 없이 현재 프로세스의 유효 사용자 ID 만 읽는다.
    if unsafe { libc::geteuid() } == 0 {
        return;
    }
    let twin = Twin::new();
    let locked = twin.source.join("a.txt");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).expect("chmod");
    let report = twin.retry();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o644)).expect("chmod");
    assert!(matches!(
        report.failed.as_slice(),
        [Failure {
            reason: Reason::SourceNotRemoved(_),
            ..
        }]
    ));
    assert_eq!(report.leftovers.len(), 1);
    assert_eq!(names(&twin.source), ["a.txt"]);
    twin.assert_copy_untouched();
}

/// 비교 중 취소하면 거기서 멈추고 아직 보지 않은 원본은 지우지 않는다.
#[test]
fn a_cancelled_retry_stops_comparing_and_keeps_the_rest() {
    let twin = Twin::new();
    let cancelled = shared();
    cancelled.cancel();
    assert!(matches!(
        same_content(
            &cancelled,
            &twin.source.join("a.txt"),
            &twin.copy.join("a.txt")
        ),
        Err(Stop::Cancelled)
    ));
    let left = Leftover::before_remove(&twin.source, &twin.copy);
    assert!(matches!(left.remove(&cancelled), Ok(None)));
    assert_eq!(names(&twin.source), ["a.txt", "sub"]);
    // 취소해도 남은 원본 카드로 돌아간다. 원본은 다시 시도할 수 있다.
    let report = run_remove_leftovers(&cancelled, None, std::slice::from_ref(&left));
    assert!(!report.cancelled);
    assert_eq!(
        report.failed,
        [Failure {
            path: twin.source.clone(),
            reason: Reason::RemoveCancelled,
        }]
    );
    assert_eq!(report.leftovers, [left]);
    assert_eq!(report.done, 1, "the copy is whole");
}

/// 취소한 뒤의 카드는 남은 원본만 센다. 이미 없는 원본은 다 지운 것으로 빼고, 손대지 못한 원본은
/// 모두 다시 시도할 항목으로 남긴다.
#[test]
fn a_cancelled_retry_lists_only_the_originals_still_there() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut lefts = Vec::new();
    for name in ["gone", "a", "b"] {
        let source = dir.path().join(name);
        let copy = dir.path().join(format!("{name}.copy"));
        std::fs::write(&source, name).expect("write");
        std::fs::write(&copy, name).expect("write");
        lefts.push(Leftover::before_remove(&source, &copy));
    }
    std::fs::remove_file(&lefts[0].source).expect("remove");
    let cancelled = shared();
    cancelled.cancel();
    let report = run_remove_leftovers(&cancelled, None, &lefts);
    assert!(!report.cancelled);
    assert_eq!((report.done, report.total), (3, 3));
    assert_eq!(report.leftovers, lefts[1..]);
    assert_eq!(
        report.failed.iter().map(|f| &f.path).collect::<Vec<_>>(),
        [&lefts[1].source, &lefts[2].source]
    );
    assert!(
        report
            .failed
            .iter()
            .all(|f| f.reason == Reason::RemoveCancelled)
    );
    assert!(report.retryable().is_empty(), "nothing is moved again");
    for left in &lefts[1..] {
        assert!(
            left.source.exists(),
            "a cancelled retry deletes nothing more"
        );
    }
}
