use super::*;
use crate::intent::{IntentOrigin, UserSource};

fn user() -> IntentOrigin {
    IntentOrigin::User {
        source: UserSource::ContextMenu,
    }
}
fn paste(paths: Vec<PathBuf>, destination: PathBuf) -> Operation {
    Operation::Paste {
        paths,
        destination,
        cut: true,
    }
}
/// 묻지 않고 Keep both 로 처리하는 작업 상태.
fn quiet() -> job::Shared {
    job::Shared::fixed(job::Choice::KeepBoth)
}
fn clipboard() -> crate::state::ExplorerClipboard {
    crate::state::ExplorerClipboard {
        identity: Arc::new(AtomicBool::new(false)),
        paths: vec!["original".into()],
        cut: true,
        source: crate::state::ExplorerPathSource::Local,
    }
}

#[test]
fn failed_or_late_cut_does_not_consume_a_new_clipboard() {
    let (mut state, engine) = crate::state::tests::test_state();
    let sid = engine.read().workspace_at(0).unwrap().all_surface_ids()[0];
    state.explorer_clipboard = Some(clipboard());
    state.request_explorer_file(
        &engine.read(),
        sid,
        paste(vec!["original".into()], "dest".into()),
        user(),
    );
    let target = state.explorer_file_requests.0.pop_front().unwrap().target;
    target.apply(&mut state, false);
    assert!(
        state.explorer_clipboard.is_some(),
        "partial failure retains the cut list"
    );
    state.request_explorer_file(
        &engine.read(),
        sid,
        paste(vec!["original".into()], "dest".into()),
        user(),
    );
    let target = state.explorer_file_requests.0.pop_front().unwrap().target;
    state.explorer_clipboard = Some(clipboard());
    let replacement = Arc::clone(&state.explorer_clipboard.as_ref().unwrap().identity);
    target.apply(&mut state, true);
    assert!(Arc::ptr_eq(
        &replacement,
        &state.explorer_clipboard.as_ref().unwrap().identity
    ));
    state.request_explorer_file(
        &engine.read(),
        sid,
        paste(vec!["original".into()], "dest".into()),
        user(),
    );
    state
        .explorer_file_requests
        .0
        .pop_front()
        .unwrap()
        .target
        .apply(&mut state, true);
    assert!(
        state.explorer_clipboard.is_none(),
        "full success consumes only its own cut list"
    );
}

#[test]
fn late_selection_and_replaced_view_are_not_modified() {
    let (mut state, engine) = crate::state::tests::test_state();
    let sid = engine.read().workspace_at(0).unwrap().all_surface_ids()[0];
    let original = crate::view::state::ViewState::default();
    state.webview_identity = original.identity();
    let dir = tempfile::tempdir().unwrap();
    let panel = crate::model::ExplorerPanel::new(sid, dir.path().into());
    state
        .explorer_views
        .get_or_init(&panel, None)
        .select_only(&dir.path().join("old"));
    state.request_explorer_file(
        &engine.read(),
        sid,
        Operation::Trash(vec![dir.path().join("old")]),
        user(),
    );
    let target = state.explorer_file_requests.0.pop_front().unwrap().target;
    assert!(target.matches_view(&original));
    assert!(!target.matches_view(&crate::view::state::ViewState::default()));
    let next = dir.path().join("new");
    state
        .explorer_views
        .get_mut(sid)
        .unwrap()
        .select_only(&next);
    target.apply(&mut state, true);
    assert!(
        state
            .explorer_views
            .get(sid)
            .unwrap()
            .selected
            .contains(&next)
    );
}

#[test]
fn admission_is_bounded_and_rejects_mirror_and_non_user_requests() {
    let (mut state, engine) = crate::state::tests::test_state();
    let sid = engine.read().workspace_at(0).unwrap().all_surface_ids()[0];
    for _ in 0..MAX_PENDING_PER_VIEW + 1 {
        state.request_explorer_file(&engine.read(), sid, Operation::Open("path".into()), user());
    }
    assert_eq!(state.explorer_file_requests.0.len(), MAX_PENDING_PER_VIEW);
    state.explorer_file_requests.0.clear();
    state.request_explorer_file(
        &engine.read(),
        sid,
        Operation::Open("x".repeat(MAX_REQUEST_BYTES + 1).into()),
        user(),
    );
    state.request_explorer_file(
        &engine.read(),
        sid,
        Operation::Open("path".into()),
        IntentOrigin::System,
    );
    assert!(state.explorer_file_requests.0.is_empty());
    let (mut state, engine) = crate::state::tests::test_mirror_state();
    let sid = engine.read().workspace_at(0).unwrap().all_surface_ids()[0];
    assert!(engine.read().is_mirror_surface(sid));
    state.request_explorer_file(
        &engine.read(),
        sid,
        Operation::Trash(vec!["path".into()]),
        user(),
    );
    assert!(state.explorer_file_requests.0.is_empty());
}

#[test]
fn a_refused_user_request_tells_the_cell_why() {
    let (mut state, engine) = crate::state::tests::test_state();
    let sid = engine.read().workspace_at(0).unwrap().all_surface_ids()[0];
    for _ in 0..MAX_PENDING_PER_VIEW {
        state.request_explorer_file(&engine.read(), sid, Operation::Open("path".into()), user());
    }
    assert!(state.toasts.messages().is_empty());
    state.request_explorer_file(&engine.read(), sid, Operation::Open("path".into()), user());
    assert_eq!(
        state.toasts.messages(),
        vec![crate::i18n::t("explorer.state.queue_full")]
    );

    let (mut state, engine) = crate::state::tests::test_state();
    let sid = engine.read().workspace_at(0).unwrap().all_surface_ids()[0];
    let many = vec![PathBuf::from("x".repeat(4096)); MAX_REQUEST_BYTES / 4096 + 1];
    state.request_explorer_file(&engine.read(), sid, Operation::Trash(many), user());
    assert!(state.explorer_file_requests.0.is_empty());
    assert_eq!(
        state.toasts.messages(),
        vec![crate::i18n::t("explorer.state.request_too_large")]
    );
}

#[test]
fn worker_copy_move_rename_preserves_partial_success() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source");
    let dest = dir.path().join("dest");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::create_dir_all(&dest).unwrap();
    let file = source.join("value");
    std::fs::write(&file, b"original bytes").unwrap();
    let operation = Operation::Paste {
        paths: vec![file.clone()],
        destination: dest.clone(),
        cut: false,
    };
    let done = std::thread::spawn(move || operation.run(&quiet()))
        .join()
        .unwrap();
    assert!(done.success(), "{done:?}");
    assert_eq!(
        std::fs::read(dest.join("value")).unwrap(),
        b"original bytes"
    );
    assert!(file.exists());
    let operation = paste(vec![file.clone(), source.join("missing")], dest.clone());
    let done = std::thread::spawn(move || operation.run(&quiet()))
        .join()
        .unwrap();
    assert!(!done.success());
    assert!(
        done.error_summary().unwrap().contains("1 succeeded"),
        "{done:?}"
    );
    assert!(!file.exists());
    assert_eq!(
        std::fs::read(dest.join("value (copy)")).unwrap(),
        b"original bytes"
    );
    let operation = Operation::Rename {
        path: dest.join("value"),
        name: "renamed".into(),
    };
    let done = std::thread::spawn(move || operation.run(&quiet()))
        .join()
        .unwrap();
    assert!(done.success(), "{done:?}");
    assert!(!dest.join("value").exists());
    assert_eq!(
        std::fs::read(dest.join("renamed")).unwrap(),
        b"original bytes"
    );
}

#[test]
fn shutdown_reports_a_running_worker_until_its_actual_join() {
    let (mut state, engine) = crate::state::tests::test_state();
    let sid = engine.read().workspace_at(0).unwrap().all_surface_ids()[0];
    state.request_explorer_file(
        &engine.read(),
        sid,
        Operation::Open("unused".into()),
        user(),
    );
    let target = state.explorer_file_requests.0.pop_front().unwrap().target;
    let (send, receive) = std::sync::mpsc::sync_channel(0);
    let worker = std::thread::spawn(move || {
        receive.recv().unwrap();
        Done::Simple(Ok(()))
    });
    let mut owner = ExplorerFiles {
        job: Some(Job {
            window: winit::window::WindowId::from(1),
            engine: engine.id,
            target,
            affected: Affected::default(),
            shared: Arc::new(quiet()),
            undo_of: None,
            seen: (0, 0, false),
            worker,
        }),
        stopping: false,
    };
    assert_eq!(owner.poll_shutdown(), 1);
    assert!(owner.stopping);
    send.send(()).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while owner.poll_shutdown() != 0 {
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert!(!owner.has_pending());
}

#[test]
fn failed_rename_preserves_original_selection() {
    let (mut state, engine) = crate::state::tests::test_state();
    let sid = engine.read().workspace_at(0).unwrap().all_surface_ids()[0];
    let dir = tempfile::tempdir().unwrap();
    let selected = dir.path().join("original");
    let panel = crate::model::ExplorerPanel::new(sid, dir.path().into());
    state
        .explorer_views
        .get_or_init(&panel, None)
        .select_only(&selected);
    state.request_explorer_file(
        &engine.read(),
        sid,
        Operation::Rename {
            path: selected.clone(),
            name: "collision".into(),
        },
        user(),
    );
    state
        .explorer_file_requests
        .0
        .pop_front()
        .unwrap()
        .target
        .apply(&mut state, false);
    assert!(
        state
            .explorer_views
            .get(sid)
            .unwrap()
            .selected
            .contains(&selected)
    );
}

#[test]
fn each_operation_names_the_folders_it_can_change_and_the_paths_it_can_remove() {
    let a = PathBuf::from("/w/src/a.txt");
    let b = PathBuf::from("/w/other/b");
    let copy = Operation::Paste {
        paths: vec![a.clone(), b.clone()],
        destination: "/w/dest".into(),
        cut: false,
    };
    assert_eq!(
        copy.affected(),
        Affected {
            changed: vec!["/w/dest".into()],
            removed: Vec::new(),
        }
    );
    assert_eq!(
        paste(vec![a.clone(), b.clone()], "/w/dest".into()).affected(),
        Affected {
            changed: vec!["/w/dest".into(), "/w/src".into(), "/w/other".into()],
            removed: vec![a.clone(), b.clone()],
        }
    );
    assert_eq!(
        Operation::Trash(vec![b.clone()]).affected(),
        Affected {
            changed: vec!["/w/other".into()],
            removed: vec![b.clone()],
        }
    );
    assert_eq!(
        Operation::Rename {
            path: a.clone(),
            name: "c.txt".into(),
        }
        .affected(),
        Affected {
            changed: vec!["/w/src".into()],
            removed: vec![a],
        }
    );
    assert_eq!(Operation::Open(b).affected(), Affected::default());
}

#[test]
fn a_finished_job_reloads_other_explorers_viewing_the_folders_it_changed() {
    let (mut state, engine) = crate::state::tests::test_state();
    let sid = engine.read().workspace_at(0).unwrap().all_surface_ids()[0];
    let dir = tempfile::tempdir().unwrap();
    let item = dir.path().join("gone.txt");
    state.request_explorer_file(
        &engine.read(),
        sid,
        Operation::Trash(vec![item.clone()]),
        user(),
    );
    let Request {
        target, operation, ..
    } = state.explorer_file_requests.0.pop_front().unwrap();
    let mut owner = ExplorerFiles {
        job: Some(Job {
            window: winit::window::WindowId::from(1),
            engine: engine.id,
            target,
            affected: operation.affected(),
            // 휴지통 이동은 실패해도 같은 경로를 다시 읽어야 한다.
            shared: Arc::new(quiet()),
            undo_of: None,
            seen: (0, 0, false),
            worker: std::thread::spawn(|| Done::Simple(Err("failed".to_string()))),
        }),
        stopping: false,
    };
    let finished = loop {
        if let Some(finished) = owner.take_finished() {
            break finished;
        }
        std::thread::yield_now();
    };
    assert!(!finished.result.success());

    // 요청한 surface 가 아닌 다른 explorer 가 같은 폴더를 보고 있다.
    let mut other = crate::adapters::ui::surface::explorer::view::ExplorerViewStore::default();
    let panel = crate::model::ExplorerPanel::new(sid + 100, dir.path().into());
    other.get_or_init(&panel, None);
    let mut unrelated = crate::adapters::ui::surface::explorer::view::ExplorerViewStore::default();
    let elsewhere = crate::model::ExplorerPanel::new(sid + 101, "/elsewhere".into());
    unrelated.get_or_init(&elsewhere, None);

    assert!(finished.reload_views(&mut other));
    assert!(!finished.reload_views(&mut unrelated));
}

#[test]
fn create_makes_a_folder_or_an_empty_file_and_never_replaces_an_entry() {
    let dir = tempfile::tempdir().unwrap();
    let create = |name: &str, folder: bool| Operation::Create {
        dir: dir.path().into(),
        name: name.into(),
        folder,
    };
    let run = |operation: Operation| match operation.run(&quiet()) {
        Done::Simple(result) => result,
        other => panic!("create answers with a simple result: {other:?}"),
    };
    assert_eq!(
        create("New folder", true).affected(),
        Affected {
            changed: vec![dir.path().into()],
            removed: Vec::new(),
        }
    );
    run(create("New folder", true)).unwrap();
    assert!(dir.path().join("New folder").is_dir());
    run(create("untitled.txt", false)).unwrap();
    assert_eq!(std::fs::read(dir.path().join("untitled.txt")).unwrap(), b"");
    std::fs::write(dir.path().join("kept"), b"bytes").unwrap();
    assert!(run(create("kept", false)).is_err());
    assert!(run(create("kept", true)).is_err());
    assert_eq!(std::fs::read(dir.path().join("kept")).unwrap(), b"bytes");
    assert!(run(create("New folder", true)).is_err());
    for name in ["", ".", "..", "a/b"] {
        assert!(run(create(name, true)).is_err(), "{name:?}");
    }
    assert!(!dir.path().join("a").exists());
}

#[test]
fn a_created_entry_is_selected_only_while_its_folder_is_still_shown() {
    let (mut state, engine) = crate::state::tests::test_state();
    let sid = engine.read().workspace_at(0).unwrap().all_surface_ids()[0];
    let dir = tempfile::tempdir().unwrap();
    let panel = crate::model::ExplorerPanel::new(sid, dir.path().into());
    state.explorer_views.get_or_init(&panel, None);
    let request = |state: &mut crate::state::MainViewState, dir: &std::path::Path| {
        state.request_explorer_file(
            &engine.read(),
            sid,
            Operation::Create {
                dir: dir.into(),
                name: "made".into(),
                folder: true,
            },
            user(),
        );
        state.explorer_file_requests.0.pop_front().unwrap().target
    };
    request(&mut state, dir.path()).apply(&mut state, true);
    let made = dir.path().join("made");
    assert!(
        state
            .explorer_views
            .get(sid)
            .unwrap()
            .selected
            .contains(&made)
    );
    let elsewhere = dir.path().join("other");
    request(&mut state, &elsewhere).apply(&mut state, true);
    assert!(
        !state
            .explorer_views
            .get(sid)
            .unwrap()
            .selected
            .contains(&elsewhere.join("made"))
    );
    request(&mut state, dir.path()).apply(&mut state, false);
    assert!(
        state
            .explorer_views
            .get(sid)
            .unwrap()
            .selected
            .contains(&made)
    );
}

#[test]
fn direct_moves_never_consume_the_clipboard() {
    let (mut state, engine) = crate::state::tests::test_state();
    let sid = engine.read().workspace_at(0).unwrap().all_surface_ids()[0];
    state.explorer_clipboard = Some(clipboard());
    state.request_explorer_file_direct(
        &engine.read(),
        sid,
        paste(vec!["original".into()], "dest".into()),
        user(),
    );
    let target = state.explorer_file_requests.0.pop_front().unwrap().target;
    assert!(target.clipboard.is_none());
    target.apply(&mut state, true);
    assert!(
        state.explorer_clipboard.is_some(),
        "a drag or retry is not a paste from the clipboard"
    );
}

#[test]
fn queued_requests_are_listed_per_surface_and_removable() {
    let (mut state, engine) = crate::state::tests::test_state();
    let sid = engine.read().workspace_at(0).unwrap().all_surface_ids()[0];
    state.request_explorer_file_direct(
        &engine.read(),
        sid,
        Operation::Paste {
            paths: vec!["a".into(), "b".into()],
            destination: "/dest/Archive".into(),
            cut: false,
        },
        user(),
    );
    state.request_explorer_file(&engine.read(), sid, Operation::Open("x".into()), user());
    state.request_explorer_file_direct(
        &engine.read(),
        sid,
        Operation::Trash(vec!["c".into()]),
        user(),
    );
    let queued = state.explorer_file_requests.queued_for(sid);
    // 열기는 진행 표시가 없어 대기열에 보이지 않는다.
    assert_eq!(queued.len(), 2);
    assert_eq!(queued[0].kind, OpKind::Copy);
    assert_eq!(queued[0].count, 2);
    assert_eq!(
        queued[0].dest.as_deref(),
        Some(std::path::Path::new("/dest/Archive"))
    );
    assert_eq!(queued[1].kind, OpKind::Trash);
    assert!(state.explorer_file_requests.queued_for(sid + 1).is_empty());
    state.explorer_file_requests.remove(queued[0].id);
    assert_eq!(state.explorer_file_requests.len(), 2);
    let left = state.explorer_file_requests.queued_for(sid);
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].id, queued[1].id);
}

#[test]
fn undo_remembers_whether_it_puts_back_a_move() {
    let moved = Operation::Undo(vec![
        UndoStep::Created("/a".into(), None),
        UndoStep::Moved {
            from: "/b".into(),
            to: "/c/b".into(),
        },
    ]);
    assert_eq!(moved.undo_of(), Some(OpKind::Move));
    assert_eq!(moved.kind(), Some(OpKind::Undo));
    let copied = Operation::Undo(vec![UndoStep::Created("/a".into(), None)]);
    assert_eq!(copied.undo_of(), Some(OpKind::Copy));
    assert_eq!(Operation::Open("x".into()).kind(), None);
}
