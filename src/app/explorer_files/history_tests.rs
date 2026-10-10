use super::*;
use crate::app::explorer_files::job::{self, Failure};
use crate::app::explorer_files::{Done, Request};
use crate::explorer_ui::view::ops::OpsAction;
use crate::intent::{IntentOrigin, UserSource};
use std::path::Path;

fn user() -> IntentOrigin {
    IntentOrigin::User {
        source: UserSource::ContextMenu,
    }
}

fn quiet() -> job::Shared {
    job::Shared::fixed(job::Choice::KeepBoth)
}

fn report(kind: OpKind, undo: Vec<UndoStep>) -> Report {
    Report {
        kind,
        dest: None,
        total: undo.len(),
        done: undo.len(),
        undo,
        failed: Vec::new(),
        skipped: Vec::new(),
        cancelled: false,
        trash_unavailable: false,
        leftovers: Vec::new(),
        unprocessed: Vec::new(),
    }
}

/// 한 항목을 모두 되돌린 결과.
fn undone() -> Report {
    let mut r = report(OpKind::Undo, Vec::new());
    r.total = 1;
    r.done = 1;
    r
}

fn source(n: usize) -> Source {
    Source {
        paths: vec![PathBuf::from(format!("/src/{n}"))],
        destination: "/dest".into(),
        cut: true,
    }
}

fn moved(n: usize) -> Vec<UndoStep> {
    vec![UndoStep::Moved {
        from: PathBuf::from(format!("/src/{n}")),
        to: PathBuf::from(format!("/dest/{n}")),
        made: None,
    }]
}

/// App 이 끝난 작업을 거둘 때처럼, 대기열 맨 앞 요청을 실행하고 그 결과를 요청한 칸의 이력에 반영한다.
fn finish_next(state: &mut crate::state::MainViewState, sid: u32) -> Report {
    let Request {
        operation,
        recorded,
        ..
    } = state
        .explorer_file_requests
        .0
        .pop_front()
        .expect("a queued request");
    let Done::Report(report) = operation.run(&quiet()) else {
        panic!("copy, move and undo report per item");
    };
    if let Some(recorded) = recorded {
        state
            .explorer_views
            .get_mut(sid)
            .expect("explorer view")
            .ops
            .history
            .record(recorded, &report);
    }
    report
}

fn history(state: &crate::state::MainViewState, sid: u32) -> &History {
    &state
        .explorer_views
        .get(sid)
        .expect("explorer view")
        .ops
        .history
}

fn explorer_state(
    dir: &Path,
) -> (
    crate::state::RequestContext,
    crate::runtime::engine_session::EngineSession,
    u32,
) {
    let (mut state, engine) = crate::state::tests::test_state();
    let sid = engine.read().workspace_at(0).unwrap().all_surface_ids()[0];
    let panel = crate::model::ExplorerPanel::new(sid, dir.into());
    state.explorer_views.get_or_init(&panel, None);
    (state, engine, sid)
}

/// 끝난 이동은 이력에 쌓이고, 되돌리면 다시 실행으로 옮겨 가며, 다시 실행은 같은 요청을 다시 보낸다.
#[test]
fn a_move_can_be_undone_and_redone_from_the_history() {
    let dir = tempfile::tempdir().unwrap();
    let (src, dest) = (dir.path().join("src"), dir.path().join("dest"));
    std::fs::create_dir_all(&src).unwrap();
    std::fs::create_dir_all(&dest).unwrap();
    std::fs::write(src.join("a.txt"), b"a").unwrap();
    let (mut state, engine, sid) = explorer_state(dir.path());

    let request = Operation::Paste {
        paths: vec![src.join("a.txt")],
        destination: dest.clone(),
        cut: true,
    };
    state.request_explorer_file_direct(&engine.read(), sid, request, user());
    finish_next(&mut state, sid);
    assert!(dest.join("a.txt").exists());
    let top = history(&state, sid).peek_undo().expect("recorded move");
    assert_eq!(top.kind(), OpKind::Move);
    assert_eq!(top.count(), 1);
    assert!(history(&state, sid).peek_redo().is_none());

    state.explorer_history_step(&engine.read(), sid, false, user(), false);
    assert!(
        history(&state, sid).peek_undo().is_none(),
        "taken when queued"
    );
    let undone = finish_next(&mut state, sid);
    assert!(undone.failed.is_empty());
    assert!(src.join("a.txt").exists() && !dest.join("a.txt").exists());
    assert_eq!(
        history(&state, sid).peek_redo().map(|e| e.source.cut),
        Some(true)
    );

    state.explorer_history_step(&engine.read(), sid, true, user(), false);
    finish_next(&mut state, sid);
    assert!(dest.join("a.txt").exists() && !src.join("a.txt").exists());
    assert!(history(&state, sid).peek_redo().is_none());
    assert_eq!(
        history(&state, sid)
            .peek_undo()
            .map(|e| e.source.paths.clone()),
        Some(vec![src.join("a.txt")])
    );
}

/// 이력이 비었을 때 되돌리기·다시 실행은 아무것도 요청하지 않는다.
#[test]
fn an_empty_history_requests_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, engine, sid) = explorer_state(dir.path());
    state.explorer_history_step(&engine.read(), sid, false, user(), false);
    state.explorer_history_step(&engine.read(), sid, true, user(), false);
    assert!(state.explorer_file_requests.0.is_empty());
    assert!(state.toasts.messages().is_empty());
}

/// 칸마다 최근 10 단계만 남고 가장 오래된 단계부터 밀려난다.
#[test]
fn the_history_keeps_the_latest_ten_steps() {
    let mut h = History::default();
    for n in 0..MAX_STEPS + 2 {
        h.record(Recorded::New(source(n)), &report(OpKind::Move, moved(n)));
    }
    let mut kept = Vec::new();
    while let Some(e) = h.take_undo() {
        kept.push(e.undo);
    }
    assert_eq!(kept.len(), MAX_STEPS);
    assert_eq!(kept[0], moved(MAX_STEPS + 1));
    assert_eq!(kept[MAX_STEPS - 1], moved(2));
}

/// 끝까지 되지 않은 작업은 이력에 쌓지 않는다. 그래도 무엇을 바꾼 새 작업은 다시 실행 목록을 비운다.
#[test]
fn only_finished_jobs_are_recorded_and_new_changes_clear_redo() {
    let mut h = History::default();
    h.record(Recorded::New(source(0)), &report(OpKind::Move, moved(0)));
    let e = h.take_undo().unwrap();
    h.record(Recorded::Undo(e), &undone());
    assert!(h.peek_redo().is_some());

    let mut partial = report(OpKind::Copy, moved(1));
    partial.failed.push(Failure {
        path: "/src/x".into(),
        reason: Reason::IntoItself,
    });
    h.record(Recorded::New(source(1)), &partial);
    assert!(h.peek_undo().is_none(), "a partial copy has no undo step");
    assert!(h.peek_redo().is_none(), "it still changed the disk");

    let mut cancelled = report(OpKind::Move, Vec::new());
    cancelled.cancelled = true;
    cancelled.done = 0;
    h.record(Recorded::New(source(2)), &report(OpKind::Move, moved(2)));
    let e = h.take_undo().unwrap();
    h.record(Recorded::Undo(e), &undone());
    h.record(Recorded::New(source(3)), &cancelled);
    assert!(h.peek_redo().is_some(), "nothing changed, redo stays");

    // 되돌리기가 다 되지 않으면 그 단계는 다시 실행할 수 없다.
    let mut h = History::default();
    h.record(Recorded::New(source(4)), &report(OpKind::Move, moved(4)));
    let e = h.take_undo().unwrap();
    let mut failed = undone();
    failed.done = 0;
    failed.failed.push(Failure {
        path: "/src/4".into(),
        reason: Reason::NewerThere,
    });
    h.record(Recorded::Undo(e), &failed);
    assert!(h.peek_redo().is_none());
}

/// 메뉴가 판단하는 "되돌릴 수 없음" 이유는 되돌리기 작업이 그 단계에 내는 이유와 같다.
#[test]
fn a_stale_reason_matches_what_undo_would_report() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let file = |name: &str| {
        let p = d.join(name);
        std::fs::write(&p, b"x").unwrap();
        p
    };
    let changed = file("changed");
    let cases = vec![
        UndoStep::Moved {
            from: d.join("free-a"),
            to: d.join("missing"),
            made: None,
        },
        UndoStep::moved(file("taken"), file("moved-b")),
        UndoStep::Replaced(file("replaced")),
        UndoStep::Created(d.join("no-copy"), None),
        UndoStep::Created(changed.clone(), Some(std::time::SystemTime::UNIX_EPOCH)),
        UndoStep::Moved {
            from: d.join("free-d"),
            to: file("moved-d"),
            made: Some(std::time::SystemTime::UNIX_EPOCH),
        },
    ];
    for step in cases {
        let entry = Entry {
            source: source(0),
            undo: vec![step.clone()],
        };
        let stale = entry.stale_reason().expect("blocked step");
        let ran = job::run_undo(&quiet(), std::slice::from_ref(&step));
        assert_eq!(ran.failed.len(), 1, "{step:?}");
        assert_eq!(ran.failed[0].reason, stale, "{step:?}");
    }

    // 되돌릴 수 있는 단계가 하나라도 있으면 행을 켠다.
    let live = UndoStep::moved(d.join("free-c"), file("moved-c"));
    let mixed = Entry {
        source: source(0),
        undo: vec![UndoStep::Replaced(file("r2")), live.clone()],
    };
    assert_eq!(mixed.stale_reason(), None);
    let ran = job::run_undo(&quiet(), &[live]);
    assert!(ran.failed.is_empty());
    assert!(d.join("free-c").exists());
}

/// 되돌릴 수 없게 된 맨 위 단계는 꺼내지 않고 그 칸에 이유를 알린다.
#[test]
fn a_stale_top_step_stays_and_tells_the_cell_why() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, engine, sid) = explorer_state(dir.path());
    let stale = UndoStep::Moved {
        from: dir.path().join("a"),
        to: dir.path().join("gone"),
        made: None,
    };
    state
        .explorer_views
        .get_mut(sid)
        .unwrap()
        .ops
        .history
        .record(Recorded::New(source(0)), &report(OpKind::Move, vec![stale]));
    state.explorer_history_step(&engine.read(), sid, false, user(), false);
    assert!(state.explorer_file_requests.0.is_empty());
    assert!(history(&state, sid).peek_undo().is_some());
    assert_eq!(
        state.toasts.messages(),
        vec![crate::i18n::t_fmt(
            "explorer.menu.undo_stale",
            crate::i18n::t("explorer.result.gone")
        )]
    );
}

/// 대기열이 가득해 요청하지 못한 단계는 이력의 제자리로 돌아간다.
#[test]
fn a_refused_step_goes_back_to_the_history() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, engine, sid) = explorer_state(dir.path());
    let to = dir.path().join("b");
    std::fs::write(&to, b"x").unwrap();
    let live = vec![UndoStep::moved(dir.path().join("a"), to)];
    let h = &mut state.explorer_views.get_mut(sid).unwrap().ops.history;
    h.record(
        Recorded::New(source(0)),
        &report(OpKind::Move, live.clone()),
    );
    h.record(Recorded::New(source(1)), &report(OpKind::Move, moved(1)));
    let e = h.take_undo().unwrap();
    h.record(Recorded::Undo(e), &undone());
    for _ in 0..crate::app::explorer_files::MAX_PENDING_PER_VIEW {
        state.request_explorer_file(&engine.read(), sid, Operation::Open("p".into()), user());
    }
    state.explorer_history_step(&engine.read(), sid, false, user(), false);
    state.explorer_history_step(&engine.read(), sid, true, user(), false);
    assert_eq!(
        state.explorer_file_requests.0.len(),
        crate::app::explorer_files::MAX_PENDING_PER_VIEW
    );
    assert_eq!(
        history(&state, sid).peek_undo().map(|e| e.undo.clone()),
        Some(live)
    );
    assert_eq!(
        history(&state, sid).peek_redo().map(|e| e.undo.clone()),
        Some(moved(1))
    );
}

/// 결과 카드의 Undo 가 이력의 단계를 되돌리면 그 단계는 이력에서 빠지고 다시 실행할 수 있다.
#[test]
fn the_card_undo_takes_its_step_out_of_the_history() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, engine, sid) = explorer_state(dir.path());
    let h = &mut state.explorer_views.get_mut(sid).unwrap().ops.history;
    h.record(Recorded::New(source(0)), &report(OpKind::Move, moved(0)));
    h.record(Recorded::New(source(1)), &report(OpKind::Move, moved(1)));
    state.apply_explorer_ops(&engine.read(), sid, OpsAction::Undo(moved(0)));
    let request = state.explorer_file_requests.0.front().expect("undo queued");
    assert!(matches!(
        &request.recorded,
        Some(Recorded::Undo(e)) if e.undo == moved(0)
    ));
    assert_eq!(
        history(&state, sid).peek_undo().map(|e| e.undo.clone()),
        Some(moved(1))
    );

    // 이력에서 밀려난 단계의 카드 Undo 는 이력과 무관하게 되돌린다.
    state.explorer_file_requests.0.clear();
    state.apply_explorer_ops(&engine.read(), sid, OpsAction::Undo(moved(7)));
    let request = state.explorer_file_requests.0.front().expect("undo queued");
    assert!(request.recorded.is_none());
}

/// 새 복사·이동이 기다리는 동안 되돌리기는 그 이전 단계를 꺼내지 않는다. 그 작업이 끝나면 그것이
/// 맨 위가 되어 다음 되돌리기가 그것을 되돌린다.
#[test]
fn undo_waits_for_a_copy_or_move_in_progress() {
    let dir = tempfile::tempdir().unwrap();
    let (src, dest) = (dir.path().join("src"), dir.path().join("dest"));
    std::fs::create_dir_all(&src).unwrap();
    std::fs::create_dir_all(&dest).unwrap();
    std::fs::write(src.join("a.txt"), b"a").unwrap();
    std::fs::write(src.join("b.txt"), b"b").unwrap();
    let (mut state, engine, sid) = explorer_state(dir.path());
    let paste = |name: &str, cut: bool| Operation::Paste {
        paths: vec![src.join(name)],
        destination: dest.clone(),
        cut,
    };
    state.request_explorer_file_direct(&engine.read(), sid, paste("a.txt", true), user());
    finish_next(&mut state, sid);

    state.request_explorer_file_direct(&engine.read(), sid, paste("b.txt", false), user());
    assert!(state.explorer_history_busy(sid));
    state.explorer_history_step(&engine.read(), sid, false, user(), false);
    assert_eq!(state.explorer_file_requests.len(), 1, "only the copy waits");
    assert_eq!(
        state.toasts.messages(),
        vec![crate::i18n::t("explorer.menu.history_busy").to_owned()]
    );
    assert_eq!(
        history(&state, sid).peek_undo().map(|e| e.source.cut),
        Some(true)
    );

    finish_next(&mut state, sid);
    assert!(!state.explorer_history_busy(sid));
    assert_eq!(
        history(&state, sid).peek_undo().map(|e| e.source.cut),
        Some(false)
    );
    state.explorer_history_step(&engine.read(), sid, false, user(), false);
    finish_next(&mut state, sid);
    assert!(!dest.join("b.txt").exists(), "the copy is undone");
    assert!(dest.join("a.txt").exists(), "the earlier move stays");
}

/// 이력에 반영할 작업이 실행 중일 때도 받지 않는다. 이력과 무관한 작업은 막지 않는다.
#[test]
fn a_running_history_job_blocks_undo_and_redo() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, engine, sid) = explorer_state(dir.path());
    let to = dir.path().join("b");
    std::fs::write(&to, b"x").unwrap();
    let live = vec![UndoStep::moved(dir.path().join("a"), to)];
    let running = |in_history| crate::explorer_ui::view::ops::Running {
        shared: std::sync::Arc::new(quiet()),
        kind: OpKind::Copy,
        dest: None,
        total: 1,
        in_history,
    };
    let view = state.explorer_views.get_mut(sid).unwrap();
    view.ops
        .history
        .record(Recorded::New(source(0)), &report(OpKind::Move, live));
    view.ops.running = Some(running(true));
    state.explorer_history_step(&engine.read(), sid, false, user(), false);
    state.explorer_history_step(&engine.read(), sid, true, user(), false);
    assert!(state.explorer_file_requests.0.is_empty());
    assert!(history(&state, sid).peek_undo().is_some());

    state.explorer_views.get_mut(sid).unwrap().ops.running = Some(running(false));
    assert!(!state.explorer_history_busy(sid));
    state.explorer_history_step(&engine.read(), sid, false, user(), false);
    assert_eq!(state.explorer_file_requests.0.len(), 1);
}

/// 이동 뒤 목적지 항목이 바뀌었으면 되돌리기는 그것을 원래 자리로 옮기지 않는다. 메뉴 행은 꺼지고
/// 단축키는 아무것도 요청하지 않는다.
#[test]
fn undo_does_not_move_back_a_destination_changed_after_the_move() {
    let dir = tempfile::tempdir().unwrap();
    let (src, dest) = (dir.path().join("src"), dir.path().join("dest"));
    std::fs::create_dir_all(&src).unwrap();
    std::fs::create_dir_all(&dest).unwrap();
    std::fs::write(src.join("a.txt"), b"a").unwrap();
    let (mut state, engine, sid) = explorer_state(dir.path());
    let request = Operation::Paste {
        paths: vec![src.join("a.txt")],
        destination: dest.clone(),
        cut: true,
    };
    state.request_explorer_file_direct(&engine.read(), sid, request, user());
    finish_next(&mut state, sid);

    let moved = dest.join("a.txt");
    std::fs::write(&moved, b"different").unwrap();
    std::fs::File::options()
        .write(true)
        .open(&moved)
        .unwrap()
        .set_modified(std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1))
        .unwrap();
    let top = history(&state, sid).peek_undo().unwrap().clone();
    assert_eq!(top.stale_reason(), Some(Reason::ChangedAfterMove));
    state.explorer_history_step(&engine.read(), sid, false, user(), false);
    assert!(state.explorer_file_requests.0.is_empty());

    let ran = job::run_undo(&quiet(), &top.undo);
    assert_eq!(ran.failed.len(), 1);
    assert_eq!(ran.failed[0].reason, Reason::ChangedAfterMove);
    assert!(!src.join("a.txt").exists(), "nothing moved back");
    assert_eq!(std::fs::read(&moved).unwrap(), b"different");
}
