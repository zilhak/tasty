//! 드래그 놓기 판정 시험. 기본 동작과 거절 이유가 디자인 결정과 같은지 본다.

use super::*;

fn p(s: &str) -> PathBuf {
    PathBuf::from(s)
}

#[test]
fn same_disk_moves_and_other_disk_copies() {
    let paths = [p("/a/x.txt")];
    let dest = p("/b");
    assert_eq!(
        verdict(&paths, &dest, true, true, false, None),
        Verdict::Go(OpKind::Move)
    );
    assert_eq!(
        verdict(&paths, &dest, false, true, false, None),
        Verdict::Go(OpKind::Copy)
    );
}

#[test]
fn the_modifier_flips_the_default() {
    let paths = [p("/a/x.txt")];
    let dest = p("/b");
    assert_eq!(
        verdict(&paths, &dest, true, true, true, None),
        Verdict::Go(OpKind::Copy)
    );
    assert_eq!(
        verdict(&paths, &dest, false, true, true, None),
        Verdict::Go(OpKind::Move)
    );
}

#[test]
fn a_folder_cannot_go_into_itself_or_its_subfolder() {
    let paths = [p("/a/src")];
    for dest in [p("/a/src"), p("/a/src/lib")] {
        assert_eq!(
            verdict(&paths, &dest, true, true, false, None),
            Verdict::No("explorer.drag.into_itself")
        );
    }
    // 이름만 앞부분이 같은 형제 폴더는 대상이 될 수 있다.
    assert_eq!(
        verdict(&paths, &p("/a/src2"), true, true, false, None),
        Verdict::Go(OpKind::Move)
    );
}

#[test]
fn moving_into_the_folder_it_is_already_in_is_refused_but_copying_is_not() {
    let paths = [p("/a/x.txt"), p("/a/y.txt")];
    let dest = p("/a");
    assert_eq!(
        verdict(&paths, &dest, true, true, false, None),
        Verdict::No("explorer.drag.same_folder")
    );
    assert_eq!(
        verdict(&paths, &dest, true, true, true, None),
        Verdict::Go(OpKind::Copy)
    );
}

#[test]
fn remote_and_read_only_targets_are_refused() {
    let paths = [p("/a/x.txt")];
    assert_eq!(
        verdict(
            &paths,
            &p("/b"),
            true,
            true,
            false,
            Some("explorer.drag.remote")
        ),
        Verdict::No("explorer.drag.remote")
    );
    assert_eq!(
        verdict(&paths, &p("/b"), true, false, false, None),
        Verdict::No("explorer.drag.no_write")
    );
}

#[test]
fn a_file_row_falls_back_to_the_shown_folder() {
    let body = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(400.0, 300.0));
    let row = |y: f32| egui::Rect::from_min_size(egui::pos2(0.0, y), egui::vec2(120.0, 24.0));
    let spots = [
        Spot {
            path: p("/shown/docs"),
            is_dir: true,
            rect: row(0.0),
            full: row(0.0),
            kind: Kind::Entry { wide: true },
        },
        Spot {
            path: p("/shown/x.txt"),
            is_dir: false,
            rect: row(24.0),
            full: row(24.0),
            kind: Kind::Entry { wide: true },
        },
    ];
    let shown = p("/shown");
    // 자세히 보기 행은 이름 셀 밖(오른쪽 열)에서도 그 행이다.
    let (dest, ring, _) = target(&spots, egui::pos2(300.0, 10.0), body, Some(&shown)).unwrap();
    assert_eq!(dest, p("/shown/docs"));
    assert_eq!(ring.width(), body.width());
    let (dest, ring, kind) = target(&spots, egui::pos2(50.0, 30.0), body, Some(&shown)).unwrap();
    assert_eq!((dest, ring, kind), (shown.clone(), body, Kind::Body));
    assert!(target(&spots, egui::pos2(500.0, 10.0), body, Some(&shown)).is_none());
}

#[test]
fn the_flip_modifier_is_matched_as_held_keys() {
    let ctrl = Combo::parse_modifiers("ctrl");
    let mods = egui::Modifiers {
        ctrl: true,
        shift: true,
        ..Default::default()
    };
    assert!(held(mods, false, ctrl));
    assert!(!held(egui::Modifiers::default(), false, ctrl));
    assert!(!held(mods, false, None));
}

/// 비-macOS 에서 `option` 반전은 Win·Super 를 누른 채 끌 때 켜진다. macOS 는 egui alt(Option)다.
#[test]
fn the_option_flip_follows_the_key_position() {
    let option = Combo::parse_modifiers("option");
    if cfg!(target_os = "macos") {
        let alt = egui::Modifiers {
            alt: true,
            ..Default::default()
        };
        assert!(held(alt, false, option));
        assert!(!held(egui::Modifiers::default(), true, option));
    } else {
        assert!(held(egui::Modifiers::default(), true, option));
        assert!(!held(egui::Modifiers::default(), false, option));
    }
}

/// 같은 컴퓨터 안에서만 놓는다. 원격 칸은 그 서버가 원격 쓰기를 받을 때만 받는다.
#[test]
fn drops_stay_on_one_computer_and_need_remote_write() {
    let mut view = ExplorerView::new();
    assert_eq!(blocked(&view, None), None);
    assert_eq!(
        blocked(&view, Some(3)),
        Some("explorer.drag.other_computer")
    );
    view.mirror_ws_id = Some(3);
    assert_eq!(blocked(&view, None), Some("explorer.drag.other_computer"));
    assert_eq!(
        blocked(&view, Some(4)),
        Some("explorer.drag.other_computer")
    );
    assert_eq!(blocked(&view, Some(3)), None);
    view.writes_allowed = false;
    assert_eq!(blocked(&view, Some(3)), Some("explorer.drag.remote"));
}

/// 원격 끌기는 서버가 알린 디스크 식별값이 같을 때만 이동을 기본으로 한다. 읽지 않은 하위 폴더는
/// 상위 폴더의 값을 쓰고, 값을 싣지 않는 옛 서버의 목록은 모르는 것으로 둔다.
#[test]
fn remote_volumes_come_from_the_listing_or_its_parent() {
    let mut view = ExplorerView::new();
    view.note_remote_volume(p("/srv/a"), Some(7));
    view.note_remote_volume(p("/mnt/b"), Some(9));
    assert_eq!(view.remote_volume_of(&p("/srv/a")), Some(7));
    assert_eq!(view.remote_volume_of(&p("/srv/a/sub")), Some(7));
    assert_eq!(view.remote_volume_of(&p("/srv/a/sub/deeper")), None);
    assert_ne!(
        view.remote_volume_of(&p("/srv/a")),
        view.remote_volume_of(&p("/mnt/b"))
    );
    view.note_remote_volume(p("/srv/a"), None);
    assert_eq!(view.remote_volume_of(&p("/srv/a")), None);
}

#[test]
fn hover_expansion_uses_the_theme_delay_and_resets_when_leaving() {
    let theme = Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0);
    let delay = theme.explorer_drag_expand_delay().to_duration();
    let ctx = egui::Context::default();
    let mut view = ExplorerView::default();
    let dir = p("/hover-folder");
    expand_after_hover(
        &ctx,
        &theme,
        &mut view,
        Some((&dir, Kind::Tree { open: false })),
    );
    assert!(!view.expanded.contains(&dir));
    view.ops.drag.hover = Some((dir.clone(), Instant::now() - delay));
    expand_after_hover(
        &ctx,
        &theme,
        &mut view,
        Some((&dir, Kind::Tree { open: false })),
    );
    assert!(view.expanded.contains(&dir));
    view.ops.drag.hover = Some((dir, Instant::now()));
    expand_after_hover(&ctx, &theme, &mut view, None);
    assert!(view.ops.drag.hover.is_none());
}

/// Windows 경로는 대소문자와 `canonicalize` 의 `\\?\` 접두사를 무시하고 비교한다.
#[test]
fn windows_paths_compare_without_case_or_verbatim_prefix() {
    assert_eq!(fold_windows(r"\\?\C:\Work\Src"), r"c:\work\src");
    assert_eq!(fold_windows(r"C:\WORK\src"), r"c:\work\src");
    assert_eq!(
        fold_windows(r"\\?\UNC\Server\Share\Dir"),
        r"\\server\share\dir"
    );
    assert_eq!(fold_windows(r"\\server\share\dir"), r"\\server\share\dir");
}

fn store_showing(sid: tasty_model::SurfaceId, dir: &Path) -> super::super::ExplorerViewStore {
    let mut views = super::super::ExplorerViewStore::default();
    let mut view = ExplorerView::new();
    view.loaded = Some((
        dir.to_path_buf(),
        tasty_model::SortColumn::Name,
        tasty_model::SortDir::Asc,
    ));
    views.views.insert(sid, view);
    views
}

/// 놓은 좌표로 정하는 OS 드롭은 칸이 보고 있는 폴더로 복사하고, 같은 판정 규칙으로 거절한다.
#[test]
fn an_os_drop_at_the_cursor_goes_to_the_shown_folder() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let shown = tmp.path().join("shown");
    std::fs::create_dir_all(&shown).expect("mkdir");
    let views = store_showing(4, &shown);
    let file = [tmp.path().join("x.txt")];
    assert_eq!(
        shown_drop(&views, 4, &file),
        Some((shown.clone(), Verdict::Go(OpKind::Copy)))
    );
    assert_eq!(
        shown_drop(&views, 4, std::slice::from_ref(&shown)),
        Some((shown.clone(), Verdict::No("explorer.drag.into_itself")))
    );
    assert_eq!(shown_drop(&views, 5, &file), None);
    let mut remote = store_showing(4, &shown);
    remote.views.get_mut(&4).expect("view").mirror_ws_id = Some(2);
    assert_eq!(
        shown_drop(&remote, 4, &file),
        Some((shown, Verdict::No("explorer.drag.other_computer")))
    );
    let mut empty = super::super::ExplorerViewStore::default();
    empty.views.insert(4, ExplorerView::new());
    assert_eq!(shown_drop(&empty, 4, &file), None);
}

/// 놓기 처리는 hover 로 정한 칸 하나를 꺼내고 모든 칸의 기록을 비운다. 두 번째 놓기는 다시 쓰지 않는다.
#[test]
fn the_hovered_os_target_is_taken_once() {
    let mut views = store_showing(1, Path::new("/one"));
    views.views.insert(2, ExplorerView::new());
    views.views.get_mut(&1).expect("view").ops.drag.os_target =
        Some((p("/one"), Verdict::Go(OpKind::Copy)));
    assert!(os_hover_claimed(&views));
    assert_eq!(
        take_os_drop(&mut views),
        Some((1, p("/one"), Verdict::Go(OpKind::Copy)))
    );
    assert!(!os_hover_claimed(&views));
    assert_eq!(take_os_drop(&mut views), None);
}
