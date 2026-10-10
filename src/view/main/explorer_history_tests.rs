use super::*;
use crate::app::explorer_files::history::{Entry, Source};
use crate::app::explorer_files::job::UndoStep;
use std::path::PathBuf;

fn entry(cut: bool, undo: Vec<UndoStep>) -> Entry {
    Entry::new(
        Source {
            paths: vec!["/src/a".into()],
            destination: "/dest".into(),
            cut,
        },
        undo,
    )
}

/// 되돌릴 수 있는 이동 단계: 옮겨 간 자리에 항목이 있고 원래 자리는 비어 있다.
fn live_move(dir: &std::path::Path, name: &str) -> UndoStep {
    let to = dir.join(name);
    std::fs::write(&to, b"x").unwrap();
    UndoStep::moved(dir.join(format!("gone-{name}")), to)
}

#[test]
fn rows_name_the_operation_and_count_and_are_absent_without_history() {
    assert!(history_items(None, None, false).is_empty());
    let dir = tempfile::tempdir().unwrap();
    let undo = entry(
        true,
        vec![live_move(dir.path(), "a"), live_move(dir.path(), "b")],
    );
    let redo = entry(false, vec![UndoStep::Created(PathBuf::from("/x"), None)]);
    let rows = history_items(Some(&undo), Some(&redo), false);
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows[0].label,
        crate::i18n::t_fmt(
            "explorer.menu.undo",
            &crate::i18n::t_count("explorer.menu.op_move", 2, &["2"])
        )
    );
    assert!(rows[0].enabled);
    assert_eq!(rows[0].tooltip, None);
    assert_eq!(
        rows[1].label,
        crate::i18n::t_fmt(
            "explorer.menu.redo",
            &crate::i18n::t_count("explorer.menu.op_copy", 1, &["1"])
        )
    );
    assert!(rows[1].enabled);
}

/// 되돌릴 수 없게 된 단계는 행을 끄고 이유를 툴팁에 적는다.
#[test]
fn a_stale_undo_row_is_disabled_with_the_reason_as_its_tooltip() {
    let dir = tempfile::tempdir().unwrap();
    let stale = entry(
        true,
        vec![UndoStep::Moved {
            from: dir.path().join("a"),
            to: dir.path().join("missing"),
            made: None,
        }],
    );
    let rows = history_items(Some(&stale), None, false);
    assert_eq!(rows.len(), 1);
    assert!(!rows[0].enabled);
    assert_eq!(
        rows[0].tooltip.as_deref(),
        Some(
            crate::i18n::t_fmt(
                "explorer.menu.undo_stale",
                crate::i18n::t("explorer.result.gone")
            )
            .as_str()
        )
    );
}

/// 행 묶음은 마지막 "구분선 · Properties" 앞에 자기 구분선과 함께 들어가 구분선 사이에 놓인다.
/// 행이 없으면 메뉴를 바꾸지 않는다.
#[test]
fn rows_go_before_the_properties_group() {
    let menu = || {
        vec![
            MenuItem::new(1, "Copy path"),
            MenuItem::separator(),
            MenuItem::new(70, "Properties"),
        ]
    };
    let mut items = menu();
    insert_rows(&mut items, Vec::new());
    assert_eq!(items.len(), 3);
    insert_rows(
        &mut items,
        vec![MenuItem::new(UNDO, "Undo"), MenuItem::new(REDO, "Redo")],
    );
    let ids: Vec<u32> = items.iter().map(|i| i.id).collect();
    assert_eq!(ids, vec![1, 0, UNDO, REDO, 0, 70]);
    assert!(items[1].is_separator() && items[4].is_separator());
}

fn explorer_state(
    dir: &std::path::Path,
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

fn user() -> crate::intent::IntentOrigin {
    crate::intent::IntentOrigin::User {
        source: crate::intent::UserSource::ContextMenu,
    }
}

fn report(
    kind: crate::app::explorer_files::job::OpKind,
    undo: Vec<UndoStep>,
) -> crate::app::explorer_files::job::Report {
    crate::app::explorer_files::job::Report {
        kind,
        dest: None,
        total: undo.len().max(1),
        done: undo.len().max(1),
        undo,
        failed: Vec::new(),
        skipped: Vec::new(),
        cancelled: false,
        trash_unavailable: false,
        leftovers: Vec::new(),
        unprocessed: Vec::new(),
    }
}

fn record(state: &mut crate::state::MainViewState, sid: u32, entry: Entry) {
    use crate::app::explorer_files::history::Recorded;
    use crate::app::explorer_files::job::OpKind;
    let report = report(OpKind::Move, entry.undo);
    state
        .explorer_views
        .get_mut(sid)
        .unwrap()
        .ops
        .history
        .record(Recorded::New(entry.source), &report);
}

/// 메뉴에서 고른 Undo 행은 그 행 이름에 적힌 단계를 되돌린다.
#[test]
fn a_picked_row_runs_the_step_its_name_shows() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, engine, sid) = explorer_state(dir.path());
    record(
        &mut state,
        sid,
        entry(true, vec![live_move(dir.path(), "a")]),
    );
    let newer = entry(
        false,
        vec![live_move(dir.path(), "b"), live_move(dir.path(), "c")],
    );
    record(&mut state, sid, newer.clone());

    let rows = history_rows(&mut state, sid);
    assert_eq!(rows[0].id, UNDO);
    assert_eq!(rows[0].label, undo_label(&newer));
    state.explorer_history_step(&engine.read(), sid, false, user(), true);
    let queued = state.explorer_file_requests.queued_steps();
    assert_eq!(queued, vec![newer.undo]);
}

/// 메뉴를 연 뒤 이력이 바뀌었으면 고른 행은 아무것도 실행하지 않고 알린다.
#[test]
fn a_pick_after_the_history_changed_runs_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, engine, sid) = explorer_state(dir.path());
    record(
        &mut state,
        sid,
        entry(true, vec![live_move(dir.path(), "a")]),
    );
    let rows = history_rows(&mut state, sid);
    assert_eq!(rows.len(), 1);
    record(
        &mut state,
        sid,
        entry(false, vec![live_move(dir.path(), "b")]),
    );

    state.explorer_history_step(&engine.read(), sid, false, user(), true);
    assert!(state.explorer_file_requests.queued_steps().is_empty());
    assert_eq!(
        state.toasts.messages(),
        vec![crate::i18n::t("explorer.menu.history_changed").to_owned()]
    );
    let history = &state.explorer_views.get(sid).unwrap().ops.history;
    assert_eq!(history.peek_undo().map(|e| e.source.cut), Some(false));
}

/// 이력에 반영할 복사·이동이 기다리는 동안 두 행은 꺼지고 그 이유를 툴팁으로 보인다.
#[test]
fn rows_are_disabled_while_a_copy_or_move_waits() {
    use crate::app::explorer_files::history::Recorded;
    use crate::app::explorer_files::job::OpKind;
    let dir = tempfile::tempdir().unwrap();
    let (mut state, engine, sid) = explorer_state(dir.path());
    record(
        &mut state,
        sid,
        entry(true, vec![live_move(dir.path(), "a")]),
    );
    record(
        &mut state,
        sid,
        entry(true, vec![live_move(dir.path(), "b")]),
    );
    let h = &mut state.explorer_views.get_mut(sid).unwrap().ops.history;
    let top = h.take_undo().unwrap();
    h.record(Recorded::Undo(top), &report(OpKind::Undo, Vec::new()));
    state.request_explorer_file_direct(
        &engine.read(),
        sid,
        crate::app::explorer_files::Operation::Paste {
            paths: vec![dir.path().join("b")],
            destination: dir.path().join("dest"),
            cut: false,
        },
        user(),
    );
    let rows = history_rows(&mut state, sid);
    let busy = crate::i18n::t("explorer.menu.history_busy");
    assert_eq!(rows.len(), 2);
    for row in &rows {
        assert!(!row.enabled, "{}", row.label);
        assert_eq!(row.tooltip.as_deref(), Some(busy));
    }
}

/// 되돌린 뒤 원본이 없어진 다시 실행 행은 꺼지고 이유를 툴팁으로 보인다.
#[test]
fn a_stale_redo_row_is_disabled_with_the_reason_as_its_tooltip() {
    use crate::app::explorer_files::history::Recorded;
    use crate::app::explorer_files::job::OpKind;
    let dir = tempfile::tempdir().unwrap();
    let (mut state, _engine, sid) = explorer_state(dir.path());
    let original = dir.path().join("orig");
    std::fs::write(&original, b"x").unwrap();
    let mut moved = entry(true, vec![live_move(dir.path(), "a")]);
    moved.source.paths = vec![original.clone()];
    record(&mut state, sid, moved);
    let h = &mut state.explorer_views.get_mut(sid).unwrap().ops.history;
    let top = h.take_undo().unwrap();
    h.record(Recorded::Undo(top), &report(OpKind::Undo, Vec::new()));
    assert!(history_rows(&mut state, sid)[0].enabled);

    std::fs::remove_file(&original).unwrap();
    let rows = history_rows(&mut state, sid);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, REDO);
    assert!(!rows[0].enabled);
    assert_eq!(
        rows[0].tooltip.as_deref(),
        Some(
            crate::i18n::t_fmt(
                "explorer.menu.redo_stale",
                crate::i18n::t("explorer.result.gone")
            )
            .as_str()
        )
    );
}
