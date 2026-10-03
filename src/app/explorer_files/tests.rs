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
fn clipboard() -> crate::state::ExplorerClipboard {
    crate::state::ExplorerClipboard {
        identity: Arc::new(AtomicBool::new(false)),
        paths: vec!["original".into()],
        cut: true,
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
    std::thread::spawn(move || operation.run())
        .join()
        .unwrap()
        .unwrap();
    assert_eq!(
        std::fs::read(dest.join("value")).unwrap(),
        b"original bytes"
    );
    assert!(file.exists());
    let operation = paste(vec![file.clone(), source.join("missing")], dest.clone());
    assert!(
        std::thread::spawn(move || operation.run())
            .join()
            .unwrap()
            .unwrap_err()
            .contains("1 succeeded")
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
    std::thread::spawn(move || operation.run())
        .join()
        .unwrap()
        .unwrap();
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
        Ok(())
    });
    let mut owner = ExplorerFiles {
        job: Some(Job {
            window: winit::window::WindowId::from(1),
            engine: engine.id,
            target,
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
