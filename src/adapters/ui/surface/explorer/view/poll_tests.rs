use super::*;
use crate::app::local_reads::LocalReads;
use tasty_model::ExplorerPanel;

/// `done` 이 참이 될 때까지 worker 를 돌린다. 끝난 job 을 거둬 동시 실행 상한에 막히지 않게 한다.
fn run_until(
    view: &mut ExplorerView,
    owner: &mut LocalReads,
    done: impl Fn(&ExplorerView) -> bool,
) {
    loop {
        owner.reap();
        owner.drive(|requests| {
            view.poll_local_reads(requests);
            true
        });
        if done(view) {
            return;
        }
        std::thread::yield_now();
    }
}

/// 패널을 그리고 읽기·확인 결과가 모두 돌아올 때까지 기다린다.
fn settle(view: &mut ExplorerView, panel: &ExplorerPanel, owner: &mut LocalReads) {
    view.sync(panel, None);
    run_until(view, owner, |v| {
        v.local_query.is_none() && v.tree_queries.is_empty() && v.poll.check.is_none()
    });
}

/// 걸린 확인이 끝날 때까지 돌린다. 그동안 다시 그리라는 신호가 있었는지 돌려준다.
fn finish_check(view: &mut ExplorerView, owner: &mut LocalReads) -> bool {
    let mut redraw = false;
    loop {
        owner.reap();
        owner.drive(|requests| {
            redraw |= view.poll_local_reads(requests);
            true
        });
        if view.poll.check.is_none() {
            return redraw;
        }
        std::thread::yield_now();
    }
}

fn shutdown(mut owner: LocalReads) {
    while owner.poll_shutdown() != 0 {
        std::thread::yield_now();
    }
}

/// 한 프레임: 프레임 시작 뒤 이 칸을 `focused` 로 그린다.
fn frame(store: &mut ExplorerViewStore, panel: &ExplorerPanel, focused: bool, now: Instant) {
    store.begin_poll_frame();
    store.get_or_init(panel, None).poll_external(focused, now);
}

#[test]
fn a_tab_without_focus_is_never_checked() {
    let panel = ExplorerPanel::new(1, PathBuf::from("/tmp/alpha"));
    let mut store = ExplorerViewStore::default();
    let t0 = Instant::now();
    for i in 0..3 {
        frame(&mut store, &panel, false, t0 + EXTERNAL_POLL_INTERVAL * i);
        let view = store.get(1).expect("view");
        assert!(view.poll.check.is_none(), "포커스가 없으면 확인하지 않는다");
    }
    assert_eq!(store.next_poll_at(), None, "깨울 이유가 없다");
}

#[test]
fn gaining_focus_checks_at_once_and_then_every_interval() {
    let panel = ExplorerPanel::new(1, PathBuf::from("/tmp/alpha"));
    let mut store = ExplorerViewStore::default();
    let t0 = Instant::now();
    frame(&mut store, &panel, false, t0);
    store.get_mut(1).expect("view").local_query = None;

    frame(&mut store, &panel, true, t0);
    let view = store.get_mut(1).expect("view");
    assert!(
        view.poll.check.is_some(),
        "포커스를 얻은 프레임에 바로 확인한다"
    );
    view.poll.check = None;
    assert_eq!(store.next_poll_at(), Some(t0 + EXTERNAL_POLL_INTERVAL));

    frame(&mut store, &panel, true, t0 + EXTERNAL_POLL_INTERVAL);
    assert!(
        store.get(1).expect("view").poll.check.is_none(),
        "주기 확인은 프레임이 시작하지 않는다"
    );
    assert!(
        !store.start_due_checks(t0 + EXTERNAL_POLL_INTERVAL / 2),
        "주기 전에는 다시 확인하지 않는다"
    );
    assert!(
        store.start_due_checks(t0 + EXTERNAL_POLL_INTERVAL),
        "주기가 지나면 그리지 않고 확인한다"
    );
    assert!(store.get(1).expect("view").poll.check.is_some());
    assert_eq!(store.next_poll_at(), Some(t0 + EXTERNAL_POLL_INTERVAL * 2));

    // 다른 탭으로 갔다가 돌아오면 주기를 기다리지 않는다.
    store.get_mut(1).expect("view").poll.check = None;
    frame(&mut store, &panel, false, t0 + EXTERNAL_POLL_INTERVAL);
    assert_eq!(store.next_poll_at(), None);
    frame(&mut store, &panel, true, t0 + EXTERNAL_POLL_INTERVAL);
    assert!(store.get(1).expect("view").poll.check.is_some());
}

#[test]
fn only_the_focused_tab_of_the_focused_window_is_checked_and_regaining_focus_checks_at_once() {
    assert!(checks_outside_changes(true, true));
    assert!(!checks_outside_changes(true, false), "창의 다른 탭");
    assert!(
        !checks_outside_changes(false, true),
        "Tasty 가 배경 앱이거나 다른 창이 포커스"
    );
    assert!(!checks_outside_changes(false, false));

    let panel = ExplorerPanel::new(1, PathBuf::from("/tmp/alpha"));
    let mut store = ExplorerViewStore::default();
    let t0 = Instant::now();
    frame(&mut store, &panel, checks_outside_changes(true, true), t0);
    store.get_mut(1).expect("view").local_query = None;
    frame(&mut store, &panel, checks_outside_changes(true, true), t0);
    store.get_mut(1).expect("view").poll.check = None;

    // 창이 포커스를 잃은 동안에는 주기가 지나도 확인하지 않고 깨우지도 않는다.
    for i in 1..4 {
        let at = t0 + EXTERNAL_POLL_INTERVAL * i;
        frame(&mut store, &panel, checks_outside_changes(false, true), at);
        assert!(store.get(1).expect("view").poll.check.is_none());
        assert_eq!(store.next_poll_at(), None);
    }
    // 창이 포커스를 되찾은 프레임에 주기를 기다리지 않고 확인하고, 그 뒤 주기를 다시 시작한다.
    let back = t0 + EXTERNAL_POLL_INTERVAL * 3 + EXTERNAL_POLL_INTERVAL / 4;
    frame(&mut store, &panel, checks_outside_changes(true, true), back);
    assert!(store.get(1).expect("view").poll.check.is_some());
    assert_eq!(store.next_poll_at(), Some(back + EXTERNAL_POLL_INTERVAL));
}

#[test]
fn a_panel_not_drawn_this_frame_stops_waking() {
    let panel = ExplorerPanel::new(1, PathBuf::from("/tmp/alpha"));
    let mut store = ExplorerViewStore::default();
    let t0 = Instant::now();
    frame(&mut store, &panel, true, t0);
    assert!(store.next_poll_at().is_some());
    store.begin_poll_frame();
    assert_eq!(store.next_poll_at(), None, "배경 탭의 칸은 그려지지 않는다");
}

#[test]
fn a_mirror_explorer_is_not_checked() {
    let panel = ExplorerPanel::new(1, PathBuf::from("/remote/project"));
    let mut view = ExplorerView::new();
    view.sync(&panel, Some(7));
    view.poll_external(true, Instant::now());
    assert!(view.poll.check.is_none());
    assert_eq!(view.poll.next_at, None);
}

#[test]
fn an_unchanged_folder_is_not_read_again() {
    let dir = tempfile::tempdir().expect("temp dir");
    std::fs::write(dir.path().join("a"), b"x").expect("write");
    let panel = ExplorerPanel::new(1, dir.path().into());
    let mut view = ExplorerView::new();
    let mut owner = LocalReads::default();
    settle(&mut view, &panel, &mut owner);
    assert_eq!(view.entries.len(), 1);

    view.poll_external(true, Instant::now());
    assert!(view.poll.check.is_some());
    assert!(
        !finish_check(&mut view, &mut owner),
        "바뀐 것이 없으면 다시 그리지 않는다"
    );
    assert!(
        view.local_query.is_none() && view.tree_queries.is_empty(),
        "표지가 같으면 목록을 읽지 않는다"
    );
    shutdown(owner);
}

#[test]
fn a_changed_folder_is_reread_while_the_old_list_stays() {
    let dir = tempfile::tempdir().expect("temp dir");
    std::fs::write(dir.path().join("keep"), b"x").expect("write");
    std::fs::write(dir.path().join("gone"), b"x").expect("write");
    let panel = ExplorerPanel::new(1, dir.path().into());
    let mut view = ExplorerView::new();
    let mut owner = LocalReads::default();
    settle(&mut view, &panel, &mut owner);
    view.select_only(&dir.path().join("keep"));
    view.toggle_select(&dir.path().join("gone"));
    let generation = view.entries_gen;

    std::thread::sleep(Duration::from_millis(20));
    std::fs::remove_file(dir.path().join("gone")).expect("remove");
    std::fs::write(dir.path().join("new"), b"x").expect("write");
    view.poll_external(true, Instant::now());
    assert!(
        !finish_check(&mut view, &mut owner),
        "확인 결과만으로는 다시 그리지 않는다"
    );
    assert!(view.local_query.is_some(), "바뀐 폴더를 바로 다시 읽는다");
    assert_eq!(
        view.entries_gen, generation,
        "읽는 동안 보이던 목록을 그대로 둔다"
    );
    assert!(matches!(view.state, super::super::LoadState::Ok));
    settle(&mut view, &panel, &mut owner);
    let mut names: Vec<&str> = view.entries.iter().map(|e| e.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(names, ["keep", "new"]);
    assert_eq!(
        view.selected,
        std::collections::HashSet::from([dir.path().join("keep")]),
        "사라진 항목만 선택에서 빠진다"
    );
    shutdown(owner);
}

#[test]
fn only_a_changed_expanded_tree_folder_is_read_again() {
    let dir = tempfile::tempdir().expect("temp dir");
    for name in ["changed", "same"] {
        std::fs::create_dir_all(dir.path().join(name).join("old")).expect("mkdir");
    }
    let panel = ExplorerPanel::new(1, dir.path().into());
    let mut view = ExplorerView::new();
    let mut owner = LocalReads::default();
    settle(&mut view, &panel, &mut owner);
    for name in ["changed", "same"] {
        let path = dir.path().join(name);
        view.expanded.insert(path.clone());
        view.tree_children_of(&path, None);
    }
    settle(&mut view, &panel, &mut owner);

    std::thread::sleep(Duration::from_millis(20));
    let changed = dir.path().join("changed");
    std::fs::create_dir(changed.join("inner")).expect("mkdir");
    view.poll_external(true, Instant::now());
    finish_check(&mut view, &mut owner);
    assert!(view.tree_queries.contains_key(&changed));
    assert!(!view.tree_queries.contains_key(&dir.path().join("same")));
    assert!(view.local_query.is_none(), "현재 폴더는 바뀌지 않았다");
    assert_eq!(
        tree_names(&view, &changed),
        ["old"],
        "읽는 동안 보이던 하위 목록을 그대로 둔다"
    );
    settle(&mut view, &panel, &mut owner);
    assert_eq!(tree_names(&view, &changed), ["inner", "old"]);
    shutdown(owner);
}

#[test]
fn the_current_folder_in_the_tree_is_read_once_and_its_node_is_replaced_in_place() {
    let dir = tempfile::tempdir().expect("temp dir");
    std::fs::create_dir(dir.path().join("old")).expect("mkdir");
    let root = dir.path().to_path_buf();
    let panel = ExplorerPanel::new(1, root.clone());
    let mut view = ExplorerView::new();
    let mut owner = LocalReads::default();
    settle(&mut view, &panel, &mut owner);
    view.expanded.insert(root.clone());
    view.tree_children_of(&root, None);
    settle(&mut view, &panel, &mut owner);

    std::thread::sleep(Duration::from_millis(20));
    std::fs::create_dir(root.join("new")).expect("mkdir");
    view.poll_external(true, Instant::now());
    finish_check(&mut view, &mut owner);
    assert!(view.local_query.is_some(), "목록은 다시 읽는다");
    assert!(
        view.tree_queries.is_empty(),
        "트리의 같은 폴더는 따로 읽지 않는다"
    );
    assert_eq!(tree_names(&view, &root), ["old"]);
    settle(&mut view, &panel, &mut owner);
    assert_eq!(tree_names(&view, &root), ["new", "old"]);
    shutdown(owner);
}

fn tree_names(view: &ExplorerView, dir: &Path) -> Vec<String> {
    view.tree_children[dir]
        .iter()
        .map(|e| e.name.clone())
        .collect()
}
