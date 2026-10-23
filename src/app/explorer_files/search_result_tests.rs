//! 하위 폴더 검색 결과에서 고른 항목의 파일 작업. 결과는 시작 폴더가 아닌 자기 폴더에 있어,
//! 작업은 그 실제 경로를 쓰고 끝나면 결과와 그 항목이 든 폴더의 목록이 함께 바뀌어야 한다.
use super::*;
use crate::adapters::ui::surface::explorer::view::{ExplorerViewStore, LoadState};
use crate::app::local_reads::LocalReads;
use crate::intent::{IntentOrigin, UserSource};
use crate::model::ExplorerPanel;

fn user() -> IntentOrigin {
    IntentOrigin::User {
        source: UserSource::ContextMenu,
    }
}

/// 목록 읽기와 하위 폴더 검색이 모두 끝날 때까지 돌린다.
fn settle(store: &mut ExplorerViewStore, owner: &mut LocalReads, panels: &[&ExplorerPanel]) {
    for panel in panels {
        store.get_or_init(panel, None);
    }
    loop {
        owner.reap();
        owner.drive(|requests| {
            store.poll_local_reads(requests);
            true
        });
        let busy = panels.iter().any(|p| {
            let view = store.get(p.id).unwrap();
            matches!(view.state, LoadState::Loading) || view.search_running()
        });
        if !busy {
            return;
        }
        std::thread::yield_now();
    }
}

fn shown(store: &ExplorerViewStore, sid: u32) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = (store.get(sid).unwrap().shown_entries())
        .into_iter()
        .map(|e| e.path)
        .collect();
    paths.sort();
    paths
}

fn names(store: &ExplorerViewStore, sid: u32) -> Vec<String> {
    let mut names: Vec<String> = (store.get(sid).unwrap().entries.iter())
        .map(|e| e.name.clone())
        .collect();
    names.sort();
    names
}

/// 앱이 하는 순서로 작업을 끝낸다: 실행, 바뀐 경로를 보는 explorer 다시 읽기, 요청한 칸 반영.
/// 다른 explorer 를 다시 읽게 했으면 true.
fn run(
    state: &mut crate::state::MainViewState,
    engine: &crate::runtime::engine_session::EngineSession,
    sid: u32,
    operation: Operation,
) -> bool {
    state.request_explorer_file(&engine.read(), sid, operation, user());
    finish_requested(state)
}

fn finish_requested(state: &mut crate::state::MainViewState) -> bool {
    let Request {
        target, operation, ..
    } = state.explorer_file_requests.0.pop_front().unwrap();
    let affected = operation.affected();
    let done =
        std::thread::spawn(move || operation.run(&job::Shared::fixed(job::Choice::KeepBoth)))
            .join()
            .unwrap();
    assert!(done.success(), "{done:?}");
    let reloaded = state
        .explorer_views
        .invalidate_local(&affected.changed, &affected.removed);
    target.finish(state, true);
    reloaded
}

fn shutdown(mut owner: LocalReads) {
    while owner.poll_shutdown() != 0 {
        std::thread::yield_now();
    }
}

/// 시작 폴더 아래 docs 폴더의 alpha.txt·keep.md, 시작 폴더의 alpha-top.txt, 빈 out 폴더.
fn tree() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let docs = dir.path().join("docs");
    std::fs::create_dir(&docs).unwrap();
    std::fs::create_dir(dir.path().join("out")).unwrap();
    std::fs::write(docs.join("alpha.txt"), b"alpha").unwrap();
    std::fs::write(docs.join("keep.md"), b"").unwrap();
    std::fs::write(dir.path().join("alpha-top.txt"), b"").unwrap();
    dir
}

#[test]
fn work_on_a_subfolder_hit_uses_its_real_path_and_refreshes_the_results_and_its_folder() {
    let (mut state, engine) = crate::state::tests::test_state();
    let sid = engine.read().workspace_at(0).unwrap().all_surface_ids()[0];
    let dir = tree();
    let root = dir.path();
    let docs = root.join("docs");
    let searching = ExplorerPanel::new(sid, root.into());
    let parent = ExplorerPanel::new(sid + 100, docs.clone());
    let panels = [&searching, &parent];
    let mut owner = LocalReads::default();
    settle(&mut state.explorer_views, &mut owner, &panels);
    state
        .explorer_views
        .get_mut(sid)
        .unwrap()
        .search_subfolders("alpha");
    settle(&mut state.explorer_views, &mut owner, &panels);
    let hit = docs.join("alpha.txt");
    assert_eq!(
        shown(&state.explorer_views, sid),
        [root.join("alpha-top.txt"), hit.clone()]
    );

    // 단축키·메뉴는 선택한 경로를 그대로 작업에 넘긴다.
    let view = state.explorer_views.get_mut(sid).unwrap();
    view.select_only(&hit);
    let picked: Vec<PathBuf> = view.selected.owned().collect();
    assert_eq!(picked, std::slice::from_ref(&hit));

    let reloaded = run(
        &mut state,
        &engine,
        sid,
        Operation::Rename {
            path: picked[0].clone(),
            name: "alpha-renamed.txt".into(),
        },
    );
    assert!(reloaded, "the explorer on the hit's folder reads it again");
    let renamed = docs.join("alpha-renamed.txt");
    assert_eq!(std::fs::read(&renamed).unwrap(), b"alpha");
    assert!(!root.join("alpha-renamed.txt").exists());
    settle(&mut state.explorer_views, &mut owner, &panels);
    assert_eq!(
        shown(&state.explorer_views, sid),
        [root.join("alpha-top.txt"), renamed.clone()]
    );
    assert_eq!(
        names(&state.explorer_views, parent.id),
        ["alpha-renamed.txt", "keep.md"]
    );

    state
        .explorer_views
        .get_mut(sid)
        .unwrap()
        .select_only(&renamed);
    let moved = root.join("out").join("alpha-renamed.txt");
    assert!(run(
        &mut state,
        &engine,
        sid,
        Operation::Paste {
            paths: vec![renamed.clone()],
            destination: root.join("out"),
            cut: true,
        },
    ));
    assert!(moved.exists() && !renamed.exists());
    settle(&mut state.explorer_views, &mut owner, &panels);
    assert_eq!(
        shown(&state.explorer_views, sid),
        [root.join("alpha-top.txt"), moved]
    );
    assert_eq!(names(&state.explorer_views, parent.id), ["keep.md"]);
    shutdown(owner);
}

#[test]
fn another_explorer_showing_results_of_an_ancestor_folder_finds_again() {
    let (mut state, engine) = crate::state::tests::test_state();
    let sid = engine.read().workspace_at(0).unwrap().all_surface_ids()[0];
    let dir = tree();
    let root = dir.path();
    let docs = root.join("docs");
    let working = ExplorerPanel::new(sid, docs.clone());
    let searching = ExplorerPanel::new(sid + 100, root.into());
    let panels = [&working, &searching];
    let mut owner = LocalReads::default();
    settle(&mut state.explorer_views, &mut owner, &panels);
    state
        .explorer_views
        .get_mut(searching.id)
        .unwrap()
        .search_subfolders("alpha");
    settle(&mut state.explorer_views, &mut owner, &panels);
    assert!(shown(&state.explorer_views, searching.id).contains(&docs.join("alpha.txt")));

    // 다른 칸에서 그 결과 항목의 이름을 바꾼다. 검색 칸이 보는 시작 폴더 자체는 바뀌지 않는다.
    run(
        &mut state,
        &engine,
        sid,
        Operation::Rename {
            path: docs.join("alpha.txt"),
            name: "alpha-2.txt".into(),
        },
    );
    settle(&mut state.explorer_views, &mut owner, &panels);
    assert_eq!(
        shown(&state.explorer_views, searching.id),
        [root.join("alpha-top.txt"), docs.join("alpha-2.txt")]
    );
    shutdown(owner);
}

#[test]
fn creating_and_pasting_while_searching_use_the_start_folder_and_reappear() {
    let (mut state, engine) = crate::state::tests::test_state();
    let sid = engine.read().workspace_at(0).unwrap().all_surface_ids()[0];
    let dir = tree();
    let root = dir.path();
    let searching = ExplorerPanel::new(sid, root.into());
    let panels = [&searching];
    let mut owner = LocalReads::default();
    settle(&mut state.explorer_views, &mut owner, &panels);
    state
        .explorer_views
        .get_mut(sid)
        .unwrap()
        .search_subfolders("alpha");
    settle(&mut state.explorer_views, &mut owner, &panels);
    state
        .explorer_views
        .get_mut(sid)
        .unwrap()
        .select_only(&root.join("docs/alpha.txt"));
    for (folder, name) in [(true, "alpha-new-folder"), (false, "alpha-new-file.rs")] {
        state.start_explorer_create(&engine.read(), sid, None, folder);
        let edit = state
            .explorer_views
            .get_mut(sid)
            .unwrap()
            .create
            .take()
            .unwrap();
        assert_eq!(
            edit.dir, root,
            "the selected hit's parent is not the destination"
        );
        assert_eq!(edit.folder, folder);
        run(
            &mut state,
            &engine,
            sid,
            Operation::Create {
                dir: edit.dir,
                name: name.into(),
                folder,
            },
        );
        settle(&mut state.explorer_views, &mut owner, &panels);
        assert!(root.join(name).exists());
        assert!(!root.join("docs").join(name).exists());
        assert!(shown(&state.explorer_views, sid).contains(&root.join(name)));
    }

    let source = tempfile::tempdir().unwrap();
    let file = source.path().join("alpha-pasted.rs");
    std::fs::write(&file, b"pasted bytes").unwrap();
    state
        .explorer_clipboard
        .set(Some(crate::state::ExplorerClipboard {
            identity: Default::default(),
            paths: vec![file.clone()],
            cut: false,
            source: crate::state::ExplorerPathSource::Local,
            os_seen: Some(crate::clipboard::files::OsClip::Other),
        }));
    let destination = state
        .explorer_views
        .get(sid)
        .unwrap()
        .shown_dir()
        .unwrap()
        .to_path_buf();
    assert_eq!(destination, root);
    state.explorer_paste(&engine.read(), sid, destination, user());
    finish_requested(&mut state);
    settle(&mut state.explorer_views, &mut owner, &panels);
    assert_eq!(
        std::fs::read(root.join("alpha-pasted.rs")).unwrap(),
        b"pasted bytes"
    );
    assert!(file.exists(), "copy keeps the source");
    assert!(!root.join("docs/alpha-pasted.rs").exists());
    assert!(shown(&state.explorer_views, sid).contains(&root.join("alpha-pasted.rs")));
    shutdown(owner);
}
