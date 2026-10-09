//! 남은 원본 다시 지우기 시험. 원본 삭제 실패는 쓰기 권한을 뺀 하위 폴더로 만든다.

use super::super::Choice;
#[cfg(unix)]
use super::super::tests::with_item;
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
