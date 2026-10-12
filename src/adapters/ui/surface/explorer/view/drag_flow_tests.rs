//! 실제 egui 입력으로 끌기를 놓기까지 돌린다. 놓기는 한 번만 요청되고, Esc 는 그 누름의 끌기를 끝내며,
//! 링크로 다른 이름을 가진 같은 폴더도 이름이 같은 폴더처럼 판정한다.

use super::*;
use crate::adapters::ui::surface::explorer::ExplorerAction;

/// 칸에 그릴 항목: 경로, 폴더 여부, 자리.
type Items = Vec<(PathBuf, bool, egui::Rect)>;

struct Pane {
    view: ExplorerView,
    body: egui::Rect,
    items: Items,
}

struct Harness {
    ctx: egui::Context,
    time: f64,
    panes: Vec<Pane>,
    drops: Vec<(usize, OpKind, Vec<PathBuf>, PathBuf)>,
}

fn cell(x: f32, y: f32) -> egui::Rect {
    egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(120.0, 24.0))
}

fn moved(pos: egui::Pos2) -> egui::Event {
    egui::Event::PointerMoved(pos)
}

fn button(pos: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    }
}

fn escape() -> egui::Event {
    egui::Event::Key {
        key: egui::Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Default::default(),
    }
}

impl Harness {
    fn new(panes: Vec<(egui::Rect, Items)>) -> Self {
        Self {
            ctx: egui::Context::default(),
            time: 0.0,
            panes: panes
                .into_iter()
                .map(|(body, items)| Pane {
                    view: ExplorerView::new(),
                    body,
                    items,
                })
                .collect(),
            drops: Vec::new(),
        }
    }

    /// 한 프레임. `discard` 면 egui 가 같은 프레임을 한 pass 더 돈다.
    fn step(&mut self, events: Vec<egui::Event>, discard: bool) {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            time: Some(self.time),
            events,
            ..Default::default()
        };
        self.time += 1.0 / 60.0;
        let theme = Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0);
        let panes = &mut self.panes;
        let drops = &mut self.drops;
        let mut discard = discard;
        // 그린 모양은 보지 않는다. 판정은 요청된 놓기와 payload 로 읽는다.
        let _ = self.ctx.run(input, |ctx| {
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show(ctx, |ui| {
                    for (index, pane) in panes.iter_mut().enumerate() {
                        let mut child =
                            ui.new_child(egui::UiBuilder::new().id_salt(index).max_rect(pane.body));
                        child.set_clip_rect(pane.body);
                        for (path, is_dir, rect) in &pane.items {
                            push(
                                &child,
                                Spot {
                                    path: path.clone(),
                                    is_dir: *is_dir,
                                    rect: *rect,
                                    full: egui::Rect::NOTHING,
                                    kind: Kind::Entry { wide: false },
                                },
                            );
                        }
                        let mut action = None;
                        frame(&child, &theme, &mut pane.view, pane.body, &mut action);
                        if let Some(ExplorerAction::Ops(OpsAction::Drop { kind, paths, dest })) =
                            action
                        {
                            drops.push((index, kind, paths, dest));
                        }
                    }
                });
            if std::mem::take(&mut discard) {
                ctx.request_discard("drag flow test");
            }
        });
    }

    fn dragging(&self) -> bool {
        egui::DragAndDrop::has_payload_of_type::<Payload>(&self.ctx)
    }

    /// `from` 에서 눌러 `to` 까지 끈다. 놓지는 않는다.
    fn drag(&mut self, from: egui::Pos2, to: egui::Pos2) {
        self.step(vec![moved(from), button(from, true)], false);
        self.step(vec![moved(from + egui::vec2(12.0, 0.0))], false);
        self.step(vec![moved(to)], false);
        self.step(vec![moved(to)], false);
        assert!(self.dragging(), "the drag must have started");
    }

    fn release(&mut self, at: egui::Pos2, discard: bool) {
        self.step(vec![moved(at), button(at, false)], discard);
    }
}

#[test]
fn a_drop_is_requested_once_even_when_the_frame_runs_again() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let file = tmp.path().join("a").join("x.txt");
    let dest = tmp.path().join("b");
    std::fs::create_dir_all(file.parent().expect("parent")).expect("mkdir a");
    std::fs::create_dir_all(&dest).expect("mkdir b");
    std::fs::write(&file, b"x").expect("write");
    let left = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(300.0, 600.0));
    let right = egui::Rect::from_min_size(egui::pos2(400.0, 0.0), egui::vec2(300.0, 600.0));
    let mut h = Harness::new(vec![
        (left, vec![(file.clone(), false, cell(10.0, 10.0))]),
        (right, vec![(dest.clone(), true, cell(410.0, 10.0))]),
    ]);
    let (a, b) = (egui::pos2(20.0, 20.0), egui::pos2(420.0, 20.0));
    h.drag(a, b);
    assert!(h.drops.is_empty(), "hovering must not drop");
    h.release(b, true);
    for _ in 0..3 {
        h.step(vec![moved(b)], false);
    }
    assert_eq!(h.drops, vec![(1, OpKind::Move, vec![file], dest)]);
    assert!(!h.dragging());
}

#[test]
fn escape_ends_the_drag_for_the_rest_of_the_press() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let file = tmp.path().join("x.txt");
    let dest = tmp.path().join("b");
    std::fs::create_dir_all(&dest).expect("mkdir b");
    std::fs::write(&file, b"x").expect("write");
    let body = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(400.0, 600.0));
    let mut h = Harness::new(vec![(
        body,
        vec![
            (file.clone(), false, cell(10.0, 10.0)),
            (dest.clone(), true, cell(10.0, 200.0)),
        ],
    )]);
    let (a, b) = (egui::pos2(20.0, 20.0), egui::pos2(20.0, 210.0));
    h.drag(a, b);
    h.step(vec![escape()], false);
    assert!(!h.dragging(), "Esc clears the payload");
    // 같은 누름을 계속 끌어도 다시 시작하지 않고, 놓아도 아무것도 하지 않는다.
    h.step(vec![moved(b + egui::vec2(4.0, 0.0))], false);
    h.step(vec![moved(b)], false);
    assert!(!h.dragging(), "the same press must not start again");
    h.release(b, false);
    assert!(h.drops.is_empty(), "{:?}", h.drops);
    // 다음 누름은 다시 끌 수 있다.
    h.drag(a, b);
    h.release(b, false);
    assert_eq!(h.drops, vec![(0, OpKind::Move, vec![file], dest)]);
}

/// 링크는 같은 폴더의 다른 이름이다. 판정은 작업 엔진처럼 링크를 푼 경로로 한다.
#[cfg(unix)]
#[test]
fn a_symlink_alias_is_judged_as_the_folder_it_points_to() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    let src = root.join("src");
    let other = root.join("other");
    std::fs::create_dir_all(src.join("sub")).expect("mkdir src");
    std::fs::create_dir_all(&other).expect("mkdir other");
    std::fs::write(src.join("x.txt"), b"x").expect("write");
    std::os::unix::fs::symlink(&src, root.join("alias")).expect("symlink");
    std::os::unix::fs::symlink(&src, root.join("link")).expect("symlink");
    let body = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(400.0, 600.0));
    let at = |row: f32| egui::pos2(20.0, 20.0 + row * 40.0);
    let spot =
        |path: PathBuf, is_dir: bool, row: f32| (path, is_dir, cell(10.0, 10.0 + row * 40.0));
    let cases = [
        // 폴더를 자기 자신의 링크 이름이나 그 아래로 넣지 않는다.
        (src.clone(), true, root.join("alias"), false),
        (src.clone(), true, root.join("alias").join("sub"), false),
        // 링크 이름으로 본 파일을 원래 폴더로 옮기는 것은 같은 폴더로의 이동이다.
        (root.join("alias").join("x.txt"), false, src.clone(), false),
        // 링크 항목 자체는 링크가 가리키는 폴더로 옮길 수 있다(엔진도 링크를 옮긴다).
        (root.join("link"), false, src.clone(), true),
        (src.clone(), true, other.clone(), true),
    ];
    for (from, from_dir, to, allowed) in cases {
        let mut h = Harness::new(vec![(
            body,
            vec![
                spot(from.clone(), from_dir, 0.0),
                spot(to.clone(), true, 2.0),
            ],
        )]);
        h.drag(at(0.0), at(2.0));
        h.release(at(2.0), false);
        let expected = if allowed {
            vec![(0, OpKind::Move, vec![from.clone()], to.clone())]
        } else {
            Vec::new()
        };
        assert_eq!(h.drops, expected, "{from:?} -> {to:?}");
    }
}
