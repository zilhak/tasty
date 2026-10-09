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
        verdict(&paths, &dest, true, true, false, false),
        Verdict::Go(OpKind::Move)
    );
    assert_eq!(
        verdict(&paths, &dest, false, true, false, false),
        Verdict::Go(OpKind::Copy)
    );
}

#[test]
fn the_modifier_flips_the_default() {
    let paths = [p("/a/x.txt")];
    let dest = p("/b");
    assert_eq!(
        verdict(&paths, &dest, true, true, true, false),
        Verdict::Go(OpKind::Copy)
    );
    assert_eq!(
        verdict(&paths, &dest, false, true, true, false),
        Verdict::Go(OpKind::Move)
    );
}

#[test]
fn a_folder_cannot_go_into_itself_or_its_subfolder() {
    let paths = [p("/a/src")];
    for dest in [p("/a/src"), p("/a/src/lib")] {
        assert_eq!(
            verdict(&paths, &dest, true, true, false, false),
            Verdict::No("explorer.drag.into_itself")
        );
    }
    // 이름만 앞부분이 같은 형제 폴더는 대상이 될 수 있다.
    assert_eq!(
        verdict(&paths, &p("/a/src2"), true, true, false, false),
        Verdict::Go(OpKind::Move)
    );
}

#[test]
fn moving_into_the_folder_it_is_already_in_is_refused_but_copying_is_not() {
    let paths = [p("/a/x.txt"), p("/a/y.txt")];
    let dest = p("/a");
    assert_eq!(
        verdict(&paths, &dest, true, true, false, false),
        Verdict::No("explorer.drag.same_folder")
    );
    assert_eq!(
        verdict(&paths, &dest, true, true, true, false),
        Verdict::Go(OpKind::Copy)
    );
}

#[test]
fn remote_and_read_only_targets_are_refused() {
    let paths = [p("/a/x.txt")];
    assert_eq!(
        verdict(&paths, &p("/b"), true, true, false, true),
        Verdict::No("explorer.drag.remote")
    );
    assert_eq!(
        verdict(&paths, &p("/b"), true, false, false, false),
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
            kind: Kind::Entry { wide: true },
        },
        Spot {
            path: p("/shown/x.txt"),
            is_dir: false,
            rect: row(24.0),
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
    assert!(held(mods, ctrl));
    assert!(!held(egui::Modifiers::default(), ctrl));
    assert!(!held(mods, None));
}
