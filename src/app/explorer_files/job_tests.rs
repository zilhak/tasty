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
    assert_eq!(report.undo, [UndoStep::Created(dst.join("a (copy).txt"))]);
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
            UndoStep::Created(dst.join("assets (copy)")),
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
        UndoStep::Created(dir.path().join("gone.txt")),
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
