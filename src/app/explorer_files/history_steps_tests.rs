//! 이름 변경·새 항목의 이력과 일부만 된 복사·이동의 이력 시험. 휴지통은 사용자 홈의 실제 휴지통이라
//! 새 항목을 실제로 되돌리지는 않고, 되돌리기 단계와 막는 판정까지만 본다.

use super::*;
use crate::app::explorer_files::named;

/// App 이 끝난 이름 변경·새 항목을 거둘 때처럼, 대기열 맨 앞 요청을 실행하고 이력에 반영한다.
fn finish_step(
    state: &mut crate::state::MainViewState,
    sid: u32,
) -> Result<Option<UndoStep>, String> {
    let Request {
        operation,
        recorded,
        ..
    } = state
        .explorer_file_requests
        .0
        .pop_front()
        .expect("a queued request");
    let Done::Step(step) = operation.run(&quiet()) else {
        panic!("rename and create return one step");
    };
    if let Some(recorded) = recorded {
        state
            .explorer_views
            .get_mut(sid)
            .expect("explorer view")
            .ops
            .history
            .record_step(recorded, step.clone().ok().flatten());
    }
    step
}

fn set_modified(path: &Path, at: std::time::SystemTime) {
    std::fs::File::options()
        .write(true)
        .open(path)
        .and_then(|f| f.set_modified(at))
        .expect("set a test modified time");
}

fn rename(path: &Path, name: &str) -> Operation {
    Operation::Rename {
        path: path.to_path_buf(),
        name: name.into(),
    }
}

/// 이름 변경은 이력에 쌓이고, 되돌리면 원래 이름으로 돌아가며, 다시 실행하면 다시 바뀐다.
#[test]
fn a_rename_can_be_undone_and_redone() {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a.txt"), dir.path().join("b.txt"));
    std::fs::write(&a, b"a").unwrap();
    let (mut state, engine, sid) = explorer_state(dir.path());

    state.request_explorer_file_direct(&engine.read(), sid, rename(&a, "b.txt"), user());
    assert!(state.explorer_history_busy(sid), "waits for the rename");
    finish_step(&mut state, sid).expect("renamed");
    assert!(b.exists() && !a.exists());
    let top = history(&state, sid).peek_undo().expect("recorded rename");
    assert_eq!(top.undo_of(), UndoOf::Rename);
    assert_eq!(top.count(), 1);

    state.explorer_history_step(&engine.read(), sid, false, user(), false);
    let undone = finish_next(&mut state, sid);
    assert!(undone.failed.is_empty(), "{:?}", undone.failed);
    assert!(a.exists() && !b.exists());
    assert_eq!(
        history(&state, sid).peek_redo().map(Entry::undo_of),
        Some(UndoOf::Rename)
    );

    state.explorer_history_step(&engine.read(), sid, true, user(), false);
    finish_step(&mut state, sid).expect("renamed again");
    assert!(b.exists() && !a.exists());
    assert!(history(&state, sid).peek_redo().is_none());
    assert_eq!(
        history(&state, sid).peek_undo().map(Entry::undo_of),
        Some(UndoOf::Rename)
    );
}

/// 원래 이름에 다른 항목이 생겼으면 되돌리지 않고, 메뉴와 단축키가 그 이유를 말한다.
#[test]
fn a_rename_is_not_undone_when_the_old_name_is_taken() {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a.txt"), dir.path().join("b.txt"));
    std::fs::write(&a, b"a").unwrap();
    let (mut state, engine, sid) = explorer_state(dir.path());
    state.request_explorer_file_direct(&engine.read(), sid, rename(&a, "b.txt"), user());
    finish_step(&mut state, sid).expect("renamed");
    std::fs::write(&a, b"another").unwrap();

    let top = history(&state, sid).peek_undo().expect("recorded rename");
    assert_eq!(top.stale_reason(), Some(Reason::NewerThere));
    assert!(
        crate::state::explorer_history::stale_text(top)
            .expect("blocked")
            .contains(crate::i18n::t("explorer.result.name_taken"))
    );
    state.explorer_history_step(&engine.read(), sid, false, user(), false);
    assert_eq!(state.explorer_file_requests.len(), 0, "nothing is queued");
    assert_eq!(std::fs::read(&a).unwrap(), b"another");
    assert_eq!(std::fs::read(&b).unwrap(), b"a");
}

/// 이름을 바꾼 뒤 항목이 바뀌었으면(수정 시각이 다르면) 되돌리지 않는다.
#[test]
fn a_rename_is_not_undone_when_the_item_changed() {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a.txt"), dir.path().join("b.txt"));
    std::fs::write(&a, b"a").unwrap();
    let (mut state, engine, sid) = explorer_state(dir.path());
    state.request_explorer_file_direct(&engine.read(), sid, rename(&a, "b.txt"), user());
    let step = finish_step(&mut state, sid)
        .expect("renamed")
        .expect("a step");
    set_modified(
        &b,
        std::time::UNIX_EPOCH + std::time::Duration::from_secs(1),
    );

    let top = history(&state, sid).peek_undo().expect("recorded rename");
    assert_eq!(top.stale_reason(), Some(Reason::ChangedAfterMove));
    let undone = job::run_undo(&quiet(), &[step]);
    assert_eq!(
        undone.failed,
        [Failure {
            path: a.clone(),
            reason: Reason::ChangedAfterMove,
        }]
    );
    assert!(b.exists() && !a.exists());
}

/// 같은 이름으로 바꾸거나 바꾸지 못한 이름 변경은 이력을 건드리지 않는다.
#[test]
fn a_rename_that_changes_nothing_is_not_recorded() {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a.txt"), dir.path().join("b.txt"));
    std::fs::write(&a, b"a").unwrap();
    std::fs::write(&b, b"b").unwrap();
    let (mut state, engine, sid) = explorer_state(dir.path());

    state.request_explorer_file_direct(&engine.read(), sid, rename(&a, "a.txt"), user());
    assert_eq!(finish_step(&mut state, sid), Ok(None));
    state.request_explorer_file_direct(&engine.read(), sid, rename(&a, "b.txt"), user());
    assert!(finish_step(&mut state, sid).is_err(), "never overwrites");

    assert!(history(&state, sid).peek_undo().is_none());
    assert_eq!(std::fs::read(&b).unwrap(), b"b");
}

/// 새 폴더·새 파일은 만든 직후 수정 시각과 함께 쌓인다. 되돌리기는 빈 그 항목을 휴지통으로 보내는 단계다.
#[test]
fn a_new_folder_and_file_are_recorded_as_created() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, engine, sid) = explorer_state(dir.path());
    for (name, folder, of) in [
        ("new", true, UndoOf::NewFolder),
        ("new.txt", false, UndoOf::NewFile),
    ] {
        let create = Operation::Create {
            dir: dir.path().to_path_buf(),
            name: name.into(),
            folder,
        };
        state.request_explorer_file_direct(&engine.read(), sid, create, user());
        let path = dir.path().join(name);
        assert_eq!(
            finish_step(&mut state, sid),
            Ok(Some(UndoStep::Made(path.clone(), job::modified_at(&path))))
        );
        let top = history(&state, sid).peek_undo().expect("recorded create");
        assert_eq!(top.undo_of(), of);
        assert_eq!(top.stale_reason(), None, "undoable while untouched");
    }
}

/// 만든 폴더 안에 항목이 생기면 바뀐 것이라 되돌리지 않고 남긴다.
#[test]
fn a_new_folder_is_not_undone_after_something_was_put_in_it() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, engine, sid) = explorer_state(dir.path());
    let create = Operation::Create {
        dir: dir.path().to_path_buf(),
        name: "new".into(),
        folder: true,
    };
    state.request_explorer_file_direct(&engine.read(), sid, create, user());
    let step = finish_step(&mut state, sid)
        .expect("created")
        .expect("a step");
    let folder = dir.path().join("new");
    // 파일시스템 시각은 수 ms 단위로 움직여 바로 넣으면 폴더 수정 시각이 같을 수 있다.
    std::thread::sleep(std::time::Duration::from_millis(50));
    std::fs::write(folder.join("inside.txt"), b"x").unwrap();

    let top = history(&state, sid).peek_undo().expect("recorded create");
    assert_eq!(top.stale_reason(), Some(Reason::ChangedSince));
    assert!(
        crate::state::explorer_history::stale_text(top)
            .expect("blocked")
            .contains(crate::i18n::t("explorer.result.changed_after_create"))
    );
    let undone = job::run_undo(&quiet(), &[step]);
    assert_eq!(undone.failed[0].reason, Reason::ChangedSince);
    assert!(folder.join("inside.txt").exists());
}

/// 되돌린 새 항목은 그 이름이 비어 있는 동안만 다시 만들 수 있다.
#[test]
fn a_new_item_is_redone_only_while_its_name_is_free() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, engine, sid) = explorer_state(dir.path());
    let create = Operation::Create {
        dir: dir.path().to_path_buf(),
        name: "new".into(),
        folder: true,
    };
    state.request_explorer_file_direct(&engine.read(), sid, create, user());
    finish_step(&mut state, sid).expect("created");
    let folder = dir.path().join("new");
    // 휴지통 대신 지워서 되돌린 것으로 둔다.
    let h = &mut state.explorer_views.get_mut(sid).unwrap().ops.history;
    let entry = h.take_undo().expect("recorded create");
    std::fs::remove_dir(&folder).unwrap();
    h.record(Recorded::Undo(entry), &undone());
    let top = h.peek_redo().expect("undone create");
    assert_eq!(top.redo_block(), None);

    std::fs::write(&folder, b"someone else's").unwrap();
    let top = history(&state, sid).peek_redo().expect("undone create");
    assert_eq!(top.redo_block(), Some(RedoBlock::Changed));
    std::fs::remove_file(&folder).unwrap();

    state.explorer_history_step(&engine.read(), sid, true, user(), false);
    finish_step(&mut state, sid).expect("created again");
    assert!(folder.is_dir());
    assert_eq!(
        history(&state, sid).peek_undo().map(Entry::undo_of),
        Some(UndoOf::NewFolder)
    );
}

/// 일부만 된 붙여넣기는 된 항목만 쌓는다. 실패·건너뜀·취소로 하지 않은 원본은 다시 실행에서 빠진다.
#[test]
fn a_partial_paste_keeps_only_the_items_that_went_through() {
    let mut h = History::default();
    let mut r = report(OpKind::Move, moved(1));
    r.total = 4;
    r.failed = vec![Failure {
        path: "/src/2".into(),
        reason: Reason::IntoItself,
    }];
    r.skipped = vec!["/src/3".into()];
    r.unprocessed = vec!["/src/4".into()];
    r.cancelled = true;
    let all = Source::Paste {
        paths: (1..=4)
            .map(|n| PathBuf::from(format!("/src/{n}")))
            .collect(),
        destination: "/dest".into(),
        cut: true,
    };
    h.record(Recorded::New(all), &r);
    let top = h.peek_undo().expect("the moved item is recorded");
    assert_eq!(top.undo, moved(1));
    assert_eq!(top.source, source(1));

    let mut none = report(OpKind::Move, Vec::new());
    none.failed = vec![Failure {
        path: "/src/5".into(),
        reason: Reason::IntoItself,
    }];
    h.record(Recorded::New(source(5)), &none);
    assert_eq!(
        h.peek_undo().map(|e| &e.undo),
        Some(&moved(1)),
        "nothing went through"
    );
}

/// 일부만 된 이동을 되돌리고 다시 실행하면 된 항목만 다시 옮긴다.
#[test]
fn a_partial_move_redoes_only_what_it_moved() {
    let dir = tempfile::tempdir().unwrap();
    let (src, dest) = (dir.path().join("src"), dir.path().join("dest"));
    std::fs::create_dir_all(&src).unwrap();
    std::fs::create_dir_all(&dest).unwrap();
    std::fs::write(src.join("a.txt"), b"a").unwrap();
    let (mut state, engine, sid) = explorer_state(dir.path());
    let request = Operation::Paste {
        paths: vec![src.join("a.txt"), src.join("missing.txt")],
        destination: dest.clone(),
        cut: true,
    };
    state.request_explorer_file_direct(&engine.read(), sid, request, user());
    let first = finish_next(&mut state, sid);
    assert_eq!((first.done, first.failed.len()), (1, 1));
    assert_eq!(history(&state, sid).peek_undo().map(Entry::count), Some(1));

    state.explorer_history_step(&engine.read(), sid, false, user(), false);
    finish_next(&mut state, sid);
    assert!(src.join("a.txt").exists());
    state.explorer_history_step(&engine.read(), sid, true, user(), false);
    let again = finish_next(&mut state, sid);
    assert_eq!((again.total, again.done), (1, 1), "{:?}", again.failed);
    assert!(again.failed.is_empty());
    assert!(dest.join("a.txt").exists() && !src.join("a.txt").exists());
}

/// 이름 변경은 진행 표시가 없어도 실행 중인 동안 이력을 바쁜 것으로 둔다. 그새 누른 되돌리기가 그 앞
/// 단계를 꺼내지 않게 한다.
#[test]
fn a_running_rename_keeps_the_history_busy() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.txt");
    std::fs::write(&a, b"a").unwrap();
    let (mut state, engine, sid) = explorer_state(dir.path());
    state.request_explorer_file_direct(&engine.read(), sid, rename(&a, "b.txt"), user());
    let Request {
        operation,
        recorded,
        ..
    } = state.explorer_file_requests.0.pop_front().unwrap();
    assert!(!state.explorer_history_busy(sid), "taken off the queue");

    let shared = std::sync::Arc::new(quiet());
    crate::app::explorer_files::ui_sync::show_running(
        &mut state,
        sid,
        &operation,
        &shared,
        recorded.is_some(),
    );
    assert!(state.explorer_history_busy(sid));
    let view = state.explorer_views.get(sid).unwrap();
    assert!(view.ops.running.is_none(), "no progress line for a rename");
}

/// 만들기와 시각 기록 사이에 다른 프로그램이 내용을 넣었으면, 기록한 시각이 지금과 같아도 되돌리지
/// 않는다. 새 항목은 비어 있을 때만 휴지통으로 보낸다.
#[test]
fn a_new_item_filled_before_its_time_was_recorded_is_not_undone() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("new.txt");
    std::fs::write(&file, b"someone else's").unwrap();
    let folder = dir.path().join("new");
    std::fs::create_dir(&folder).unwrap();
    std::fs::write(folder.join("inside.txt"), b"x").unwrap();
    for path in [&file, &folder] {
        let step = UndoStep::Made(path.clone(), job::modified_at(path));
        assert_eq!(blocked(&step), Some(Reason::ChangedSince), "{path:?}");
        let undone = job::run_undo(&quiet(), &[step]);
        assert_eq!(undone.failed[0].reason, Reason::ChangedSince);
        assert!(path.exists());
    }
}

/// 수정 시각 단위가 거친 파일시스템에서는 만든 직후 쓴 내용이 같은 시각 칸에 들어갈 수 있다. 시각이
/// 같아도 길이가 0 이 아니면 바뀐 것으로 본다.
#[test]
fn a_new_file_written_within_the_same_time_tick_is_not_undone() {
    let dir = tempfile::tempdir().unwrap();
    let step = named::create(dir.path(), "new.txt", false)
        .expect("created")
        .expect("a step");
    let UndoStep::Made(path, Some(made)) = &step else {
        panic!("a new item step with a time: {step:?}");
    };
    std::fs::write(path, b"typed right away").unwrap();
    set_modified(path, *made);
    assert_eq!(job::modified_at(path), Some(*made), "same time tick");
    assert_eq!(blocked(&step), Some(Reason::ChangedSince));
    let undone = job::run_undo(&quiet(), std::slice::from_ref(&step));
    assert_eq!(undone.failed[0].reason, Reason::ChangedSince);
    assert_eq!(std::fs::read(path).unwrap(), b"typed right away");
}

/// 대소문자를 무시하는 파일시스템에서는 대소문자만 바꾼 이름 변경 뒤 원래 이름이 바뀐 항목 자신을
/// 가리킨다. 같은 파일(dev·inode)이고 이름이 대소문자만 다르면 원래 자리는 비어 있는 것으로 본다.
/// 이 시험은 대소문자를 구분하는 파일시스템에서 하드 링크로 같은 파일을 가리키는 원래 이름을 흉내 낸다.
#[cfg(unix)]
#[test]
fn a_case_only_rename_does_not_see_itself_at_the_old_name() {
    let dir = tempfile::tempdir().unwrap();
    let (lower, upper) = (dir.path().join("a.txt"), dir.path().join("A.txt"));
    std::fs::write(&upper, b"a").unwrap();
    std::fs::hard_link(&upper, &lower).unwrap();
    let step = UndoStep::Moved {
        from: lower.clone(),
        to: upper.clone(),
        made: job::modified_at(&upper),
    };
    assert!(!named::taken(&lower, &upper));
    assert_eq!(blocked(&step), None);

    // 같은 파일이어도 이름이 대소문자 이상으로 다르면 다른 항목이 차지한 것이다.
    let other = dir.path().join("b.txt");
    std::fs::hard_link(&upper, &other).unwrap();
    assert!(named::taken(&other, &upper));
    // 대소문자만 다른 이름이어도 다른 파일이면 차지한 것이다.
    std::fs::remove_file(&lower).unwrap();
    std::fs::write(&lower, b"another").unwrap();
    assert!(named::taken(&lower, &upper));
    assert_eq!(blocked(&step), Some(Reason::NewerThere));
}
