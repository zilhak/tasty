//! 파일 작업 엔진 시험. 휴지통은 사용자 홈의 실제 휴지통이라 여기서 부르지 않는다.

use super::*;
use std::sync::{Arc, OnceLock, Weak};

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).expect("read a test file")
}

fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .expect("list a test folder")
        .map(|e| {
            e.expect("read a test entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    names
}

/// 질문이 올라올 때까지 기다린다.
fn wait_ask(shared: &Shared) -> Ask {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(ask) = shared.pending_ask() {
            return ask;
        }
        assert!(Instant::now() < deadline, "no conflict question arrived");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn spawn_transfer(
    shared: &Arc<Shared>,
    sources: Vec<PathBuf>,
    dest: &Path,
    cut: bool,
) -> std::thread::JoinHandle<Report> {
    let shared = Arc::clone(shared);
    let dest = dest.to_path_buf();
    std::thread::spawn(move || run_transfer(&shared, &sources, &dest, cut))
}

#[test]
fn keep_both_is_the_default_and_never_overwrites() {
    let dir = tempfile::tempdir().expect("tempdir");
    let src = dir.path().join("src");
    let dst = dir.path().join("dst");
    std::fs::create_dir_all(&src).expect("mkdir");
    std::fs::create_dir_all(&dst).expect("mkdir");
    std::fs::write(src.join("a.txt"), "incoming").expect("write");
    std::fs::write(dst.join("a.txt"), "existing").expect("write");

    let report = run_transfer(
        &Shared::fixed(Choice::KeepBoth),
        &[src.join("a.txt")],
        &dst,
        false,
    );

    assert_eq!(report.done, 1);
    assert!(report.failed.is_empty());
    assert_eq!(read(&dst.join("a.txt")), "existing");
    assert_eq!(read(&dst.join("a (copy).txt")), "incoming");
    assert_eq!(names(&dst), ["a (copy).txt", "a.txt"]);
    let dst = dst.canonicalize().expect("canonical");
    assert_eq!(report.undo, [UndoStep::created(dst.join("a (copy).txt"))]);
}

#[test]
fn replace_overwrites_files_but_never_folders() {
    let dir = tempfile::tempdir().expect("tempdir");
    let src = dir.path().join("src");
    let dst = dir.path().join("dst");
    std::fs::create_dir_all(src.join("assets")).expect("mkdir");
    std::fs::create_dir_all(dst.join("assets")).expect("mkdir");
    std::fs::write(src.join("a.txt"), "incoming").expect("write");
    std::fs::write(dst.join("a.txt"), "existing").expect("write");
    std::fs::write(src.join("assets/new.txt"), "n").expect("write");
    std::fs::write(dst.join("assets/old.txt"), "o").expect("write");

    let report = run_transfer(
        &Shared::fixed(Choice::Replace),
        &[src.join("a.txt"), src.join("assets")],
        &dst,
        false,
    );

    assert_eq!(report.done, 2);
    assert_eq!(read(&dst.join("a.txt")), "incoming");
    // 폴더는 합치지도 바꾸지도 않고 Keep both 로 둔다.
    assert_eq!(names(&dst.join("assets")), ["old.txt"]);
    assert_eq!(names(&dst.join("assets (copy)")), ["new.txt"]);
    let dst = dst.canonicalize().expect("canonical");
    assert_eq!(
        report.undo,
        [
            UndoStep::Replaced(dst.join("a.txt")),
            UndoStep::created(dst.join("assets (copy)")),
        ]
    );
}

#[test]
fn apply_all_answers_the_remaining_conflicts_once() {
    let dir = tempfile::tempdir().expect("tempdir");
    let src = dir.path().join("src");
    let dst = dir.path().join("dst");
    std::fs::create_dir_all(&src).expect("mkdir");
    std::fs::create_dir_all(&dst).expect("mkdir");
    for name in ["a.txt", "b.txt", "c.txt"] {
        std::fs::write(src.join(name), "incoming").expect("write");
        std::fs::write(dst.join(name), "existing").expect("write");
    }
    let shared = Arc::new(Shared::interactive(|| {}));
    let worker = spawn_transfer(
        &shared,
        vec![src.join("a.txt"), src.join("b.txt"), src.join("c.txt")],
        &dst,
        false,
    );

    let ask = wait_ask(&shared);
    assert_eq!(ask.name, "a.txt");
    assert!(!ask.folder_conflict);
    assert_eq!(ask.remaining, 2);
    shared.answer(Answer {
        choice: Choice::Skip,
        apply_all: true,
    });
    let report = worker.join().expect("worker");

    assert_eq!(report.skipped.len(), 3);
    assert_eq!(report.done, 0);
    for name in ["a.txt", "b.txt", "c.txt"] {
        assert_eq!(read(&dst.join(name)), "existing");
    }
    assert_eq!(names(&dst), ["a.txt", "b.txt", "c.txt"]);
    assert_eq!(report.retryable().len(), 3);
}

#[test]
fn replace_for_all_still_asks_again_on_a_folder_conflict() {
    let dir = tempfile::tempdir().expect("tempdir");
    let src = dir.path().join("src");
    let dst = dir.path().join("dst");
    std::fs::create_dir_all(src.join("assets")).expect("mkdir");
    std::fs::create_dir_all(dst.join("assets")).expect("mkdir");
    std::fs::write(src.join("a.txt"), "incoming").expect("write");
    std::fs::write(dst.join("a.txt"), "existing").expect("write");
    let shared = Arc::new(Shared::interactive(|| {}));
    let worker = spawn_transfer(
        &shared,
        vec![src.join("a.txt"), src.join("assets")],
        &dst,
        false,
    );

    assert!(!wait_ask(&shared).folder_conflict);
    shared.answer(Answer {
        choice: Choice::Replace,
        apply_all: true,
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    let ask = loop {
        if let Some(ask) = shared.pending_ask().filter(|a| a.name == "assets") {
            break ask;
        }
        assert!(
            Instant::now() < deadline,
            "the folder conflict was not asked"
        );
        std::thread::sleep(Duration::from_millis(5));
    };
    assert!(ask.folder_conflict);
    shared.answer(Answer {
        choice: Choice::Skip,
        apply_all: false,
    });
    let report = worker.join().expect("worker");

    assert_eq!(read(&dst.join("a.txt")), "incoming");
    assert_eq!(report.skipped, [src.join("assets")]);
    assert_eq!(names(&dst), ["a.txt", "assets"]);
}

#[test]
fn cancel_while_waiting_for_an_answer_changes_nothing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let src = dir.path().join("src");
    let dst = dir.path().join("dst");
    std::fs::create_dir_all(&src).expect("mkdir");
    std::fs::create_dir_all(&dst).expect("mkdir");
    std::fs::write(src.join("a.txt"), "incoming").expect("write");
    std::fs::write(dst.join("a.txt"), "existing").expect("write");
    let shared = Arc::new(Shared::interactive(|| {}));
    let worker = spawn_transfer(&shared, vec![src.join("a.txt")], &dst, true);

    wait_ask(&shared);
    shared.cancel();
    let report = worker.join().expect("worker");

    assert!(report.cancelled);
    assert_eq!(report.done, 0);
    assert_eq!(read(&dst.join("a.txt")), "existing");
    assert_eq!(read(&src.join("a.txt")), "incoming");
    assert_eq!(names(&dst), ["a.txt"]);
}

#[test]
fn cancel_during_a_copy_leaves_no_partial_entry() {
    let dir = tempfile::tempdir().expect("tempdir");
    let src = dir.path().join("big.bin");
    let dst = dir.path().join("dst");
    std::fs::create_dir_all(&dst).expect("mkdir");
    std::fs::write(&src, vec![7u8; COPY_CHUNK * 4]).expect("write");
    // 첫 진행 알림에서 취소한다. 알림은 시작 뒤 WAKE_INTERVAL 이 지나야 나가므로 그만큼 기다린 뒤 시작한다.
    let me: Arc<OnceLock<Weak<Shared>>> = Arc::default();
    let hook = Arc::clone(&me);
    let shared = Arc::new(Shared::interactive(move || {
        if let Some(shared) = hook.get().and_then(Weak::upgrade) {
            shared.cancel();
        }
    }));
    assert!(me.set(Arc::downgrade(&shared)).is_ok());
    std::thread::sleep(WAKE_INTERVAL + Duration::from_millis(50));

    let report = run_transfer(&shared, std::slice::from_ref(&src), &dst, false);

    assert!(report.cancelled);
    assert_eq!(report.done, 0);
    assert!(report.undo.is_empty());
    assert!(
        names(&dst).is_empty(),
        "staging left behind: {:?}",
        names(&dst)
    );
    assert_eq!(
        std::fs::metadata(&src).expect("source").len(),
        (COPY_CHUNK * 4) as u64
    );
}

#[test]
fn moving_into_the_same_folder_changes_nothing() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("a.txt"), "a").expect("write");
    let report = run_transfer(
        &Shared::fixed(Choice::KeepBoth),
        &[dir.path().join("a.txt")],
        dir.path(),
        true,
    );
    assert_eq!(report.done, 1);
    assert!(report.undo.is_empty());
    assert_eq!(names(dir.path()), ["a.txt"]);
}

#[test]
fn a_folder_cannot_go_inside_itself() {
    let dir = tempfile::tempdir().expect("tempdir");
    let folder = dir.path().join("f");
    std::fs::create_dir_all(folder.join("sub")).expect("mkdir");
    for cut in [false, true] {
        let report = run_transfer(
            &Shared::fixed(Choice::KeepBoth),
            std::slice::from_ref(&folder),
            &folder.join("sub"),
            cut,
        );
        assert_eq!(report.failed.len(), 1);
        assert_eq!(report.failed[0].reason, Reason::IntoItself);
        assert_eq!(names(&folder.join("sub")), Vec::<String>::new());
    }
}

#[test]
fn undo_puts_a_moved_entry_back() {
    let dir = tempfile::tempdir().expect("tempdir");
    let dst = dir.path().join("dst");
    std::fs::create_dir_all(&dst).expect("mkdir");
    std::fs::write(dir.path().join("a.txt"), "a").expect("write");
    let moved = run_transfer(
        &Shared::fixed(Choice::KeepBoth),
        &[dir.path().join("a.txt")],
        &dst,
        true,
    );
    assert_eq!(names(&dst), ["a.txt"]);

    let undone = run_undo(&Shared::fixed(Choice::KeepBoth), &moved.undo);

    assert_eq!(undone.done, 1);
    assert!(undone.failed.is_empty());
    assert_eq!(read(&dir.path().join("a.txt")), "a");
    assert!(names(&dst).is_empty());
}

#[test]
fn undo_leaves_a_newer_entry_in_place() {
    let dir = tempfile::tempdir().expect("tempdir");
    let dst = dir.path().join("dst");
    std::fs::create_dir_all(&dst).expect("mkdir");
    std::fs::write(dir.path().join("a.txt"), "old").expect("write");
    let moved = run_transfer(
        &Shared::fixed(Choice::KeepBoth),
        &[dir.path().join("a.txt")],
        &dst,
        true,
    );
    std::fs::write(dir.path().join("a.txt"), "newer").expect("write");

    let undone = run_undo(&Shared::fixed(Choice::KeepBoth), &moved.undo);

    assert_eq!(undone.done, 0);
    assert_eq!(undone.failed[0].reason, Reason::NewerThere);
    assert_eq!(read(&dir.path().join("a.txt")), "newer");
    assert_eq!(read(&dst.join("a.txt")), "old");
}

#[test]
fn undo_reports_what_it_cannot_restore() {
    let dir = tempfile::tempdir().expect("tempdir");
    let steps = [
        UndoStep::Replaced(dir.path().join("r.txt")),
        UndoStep::Created(dir.path().join("gone.txt"), None),
    ];
    let undone = run_undo(&Shared::fixed(Choice::KeepBoth), &steps);
    // 뒤에서부터 되돌린다.
    assert_eq!(
        undone.failed,
        [
            Failure {
                path: dir.path().join("gone.txt"),
                reason: Reason::Gone,
            },
            Failure {
                path: dir.path().join("r.txt"),
                reason: Reason::Replaced,
            },
        ]
    );
}

#[test]
fn retry_skips_entries_whose_copy_already_exists() {
    let mut report = Report::new(OpKind::Move, None, 3);
    report.failed = vec![
        Failure {
            path: PathBuf::from("/a"),
            reason: Reason::Os("denied".into()),
        },
        Failure {
            path: PathBuf::from("/b"),
            reason: Reason::SourceNotRemoved("busy".into()),
        },
    ];
    report.skipped = vec![PathBuf::from("/c")];
    assert_eq!(
        report.retryable(),
        [PathBuf::from("/a"), PathBuf::from("/c")]
    );
}

#[test]
fn copy_reports_bytes_and_keeps_links_as_links() {
    let dir = tempfile::tempdir().expect("tempdir");
    let src = dir.path().join("tree");
    let dst = dir.path().join("dst");
    std::fs::create_dir_all(src.join("inner")).expect("mkdir");
    std::fs::create_dir_all(&dst).expect("mkdir");
    std::fs::write(src.join("inner/x.txt"), "12345").expect("write");
    #[cfg(unix)]
    std::os::unix::fs::symlink("inner/x.txt", src.join("link")).expect("symlink");
    let shared = Shared::fixed(Choice::KeepBoth);

    let report = run_transfer(&shared, std::slice::from_ref(&src), &dst, false);

    assert_eq!(report.done, 1);
    assert_eq!(read(&dst.join("tree/inner/x.txt")), "12345");
    #[cfg(unix)]
    assert!(
        dst.join("tree/link")
            .symlink_metadata()
            .expect("link")
            .file_type()
            .is_symlink()
    );
    let snap = shared.snapshot();
    assert_eq!(snap.items_done, 1);
    assert_eq!(snap.items_total, 1);
    assert_eq!(snap.bytes_done, snap.bytes_total);
}

/// 질문 없이 한 항목을 다루는 Item. 경합·원본 삭제 실패처럼 run 으로 만들기 어려운 경로를 직접 부른다.
pub(super) fn with_item<R>(
    shared: &Shared,
    dest: &Path,
    cut: bool,
    f: impl FnOnce(&mut Item<'_>) -> R,
) -> R {
    let mut sticky = None;
    let mut left = 0;
    let mut item = Item {
        shared,
        dest,
        shown_dest: dest,
        cut,
        sticky: &mut sticky,
        conflicts_left: &mut left,
        base: 0,
    };
    f(&mut item)
}

/// 이름을 확인한 뒤 공개하기 전에 같은 이름이 생기면, 공개는 그것을 덮어쓰지 않고 Keep both 이름으로 간다.
#[test]
fn publishing_never_overwrites_a_name_that_appears_late() {
    let dir = tempfile::tempdir().expect("tempdir");
    let src = dir.path().join("src");
    let dst = dir.path().join("dst");
    std::fs::create_dir_all(&src).expect("mkdir");
    std::fs::create_dir_all(&dst).expect("mkdir");
    std::fs::write(src.join("a.txt"), "incoming").expect("write");
    std::fs::write(src.join("b.txt"), "moving").expect("write");
    // 다른 프로그램이 확인 뒤에 만든 항목.
    std::fs::write(dst.join("a.txt"), "late").expect("write");
    std::fs::write(dst.join("b.txt"), "late").expect("write");
    let shared = Shared::fixed(Choice::KeepBoth);

    let a = src.join("a.txt");
    let meta = a.symlink_metadata().expect("meta");
    let copied = with_item(&shared, &dst, false, |item| {
        item.copy(&a, &meta, dst.join("a.txt"), Publish::NoReplace)
    });
    assert!(matches!(
        copied,
        Ok(Outcome::Done(UndoStep::Created(ref p, _))) if *p == dst.join("a (copy).txt")
    ));

    let mut target = dst.join("b.txt");
    let moved = with_item(&shared, &dst, true, |item| {
        item.rename(&src.join("b.txt"), &mut target, Publish::NoReplace)
    });
    assert!(matches!(
        moved,
        Ok(Some(Outcome::Done(UndoStep::Moved { .. })))
    ));
    assert_eq!(target, dst.join("b (copy).txt"));

    assert_eq!(read(&dst.join("a.txt")), "late");
    assert_eq!(read(&dst.join("b.txt")), "late");
    assert_eq!(read(&dst.join("a (copy).txt")), "incoming");
    assert_eq!(read(&dst.join("b (copy).txt")), "moving");
}

/// 다른 디스크 이동에서 원본을 일부만 지웠으면 사본은 하나뿐인 온전한 자료다.
/// 보고는 실패로 남고 되돌리기 단계가 없어 Undo 가 사본을 휴지통으로 보내지 않는다.
#[cfg(unix)]
#[test]
fn a_partly_removed_source_keeps_the_copy_out_of_undo() {
    use std::os::unix::fs::PermissionsExt;
    // root 는 권한을 무시해 원본이 다 지워진다. 이 경우 재현할 수 없다.
    // SAFETY: geteuid 는 인자 없이 현재 프로세스의 유효 사용자 ID 만 읽는다.
    if unsafe { libc::geteuid() } == 0 {
        return;
    }
    let dir = tempfile::tempdir().expect("tempdir");
    let payload = dir.path().join("src").join("payload");
    let dst = dir.path().join("dst");
    std::fs::create_dir_all(payload.join("ro")).expect("mkdir");
    std::fs::create_dir_all(&dst).expect("mkdir");
    for name in ["a1.txt", "a2.txt", "a3.txt"] {
        std::fs::write(payload.join(name), name).expect("write");
    }
    std::fs::write(payload.join("ro").join("b.txt"), "b").expect("write");
    let lock = |mode| {
        std::fs::set_permissions(payload.join("ro"), std::fs::Permissions::from_mode(mode))
            .expect("chmod")
    };
    lock(0o555);

    let shared = Shared::fixed(Choice::KeepBoth);
    let meta = payload.symlink_metadata().expect("meta");
    let result = with_item(&shared, &dst, true, |item| {
        item.copy(&payload, &meta, dst.join("payload"), Publish::NoReplace)
    });
    let mut report = Report::new(OpKind::Move, Some(dst.clone()), 1);
    let flow = record(&mut report, &shared, &payload, result);
    lock(0o755);

    assert!(flow.is_continue());
    assert_eq!(report.done, 1);
    assert!(
        report.undo.is_empty(),
        "the only full copy must not be undone"
    );
    assert!(matches!(
        report.failed.as_slice(),
        [Failure {
            reason: Reason::SourceNotRemoved(_),
            ..
        }]
    ));
    assert!(report.retryable().is_empty());
    // 사본은 온전하다. 원본은 지우지 못한 ro 를 포함해 남는다(그 밖에 얼마나 지워졌는지는 읽는 순서에 달렸다).
    assert_eq!(
        names(&dst.join("payload")),
        ["a1.txt", "a2.txt", "a3.txt", "ro"]
    );
    assert_eq!(names(&dst.join("payload").join("ro")), ["b.txt"]);
    assert_eq!(names(&payload.join("ro")), ["b.txt"]);
    assert!(run_undo(&shared, &report.undo).failed.is_empty());
    assert_eq!(names(&dst.join("payload")).len(), 4);
}

/// 작업 뒤 수정 시각이 바뀐 사본은 사용자가 고친 것으로 보고 되돌리기에서 남긴다.
#[test]
fn undo_keeps_a_copy_changed_after_the_job() {
    let dir = tempfile::tempdir().expect("tempdir");
    let src = dir.path().join("src");
    let dst = dir.path().join("dst");
    std::fs::create_dir_all(&src).expect("mkdir");
    std::fs::create_dir_all(&dst).expect("mkdir");
    std::fs::write(src.join("a.txt"), "copied").expect("write");
    let report = run_transfer(
        &Shared::fixed(Choice::KeepBoth),
        &[src.join("a.txt")],
        &dst,
        false,
    );
    let copy = dst.canonicalize().expect("canonical").join("a.txt");
    assert!(matches!(report.undo.as_slice(), [UndoStep::Created(p, Some(_))] if *p == copy));

    std::fs::write(&copy, "edited").expect("edit the copy");
    let later = SystemTime::now() + Duration::from_secs(60);
    std::fs::File::options()
        .write(true)
        .open(&copy)
        .and_then(|f| f.set_modified(later))
        .expect("touch the copy");

    let undone = run_undo(&Shared::fixed(Choice::KeepBoth), &report.undo);
    assert_eq!(
        undone.failed,
        [Failure {
            path: copy.clone(),
            reason: Reason::ChangedSince,
        }]
    );
    assert_eq!(read(&copy), "edited");
}
