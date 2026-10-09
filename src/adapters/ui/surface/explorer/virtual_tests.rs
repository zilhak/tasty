//! 목록을 화면에 걸친 항목만 그리는지 세 보기 모드에서 시험한다. 화면 밖 항목으로 스크롤해
//! 고르기, 타입어헤드 이동, 스크롤해도 유지되는 이름 입력 줄도 함께 본다.

use std::collections::HashSet;
use std::path::PathBuf;

use egui::{Event, Modifiers, PointerButton, Pos2, RawInput, Rect};
use tasty_model::{ExplorerPanel, ExplorerViewMode};

const N: usize = 20_000;
const DIR: &str = "/tasty-test-no-such-dir";
const CELL: Rect = Rect::from_min_max(Pos2::ZERO, Pos2::new(1100.0, 600.0));
const MODES: [ExplorerViewMode; 3] = [
    ExplorerViewMode::Detail,
    ExplorerViewMode::List,
    ExplorerViewMode::Grid,
];

fn name(i: usize) -> String {
    format!("item{i:05}.txt")
}

fn path(i: usize) -> PathBuf {
    PathBuf::from(format!("{DIR}/{}", name(i)))
}

struct Harness {
    ctx: egui::Context,
    panel: ExplorerPanel,
    view: super::ExplorerView,
    focused: bool,
}

impl Harness {
    fn new(mode: ExplorerViewMode) -> Self {
        crate::i18n::init("en");
        let mut panel = ExplorerPanel::new(1, PathBuf::from(DIR));
        panel.active_tab_mut().view_mode = mode;
        let mut view = super::ExplorerView::new();
        // 만든 항목 고르기는 그 폴더를 보고 있을 때만 동작한다. 읽기를 시작해 폴더를 정하고
        // 읽기 결과 대신 항목을 직접 넣는다.
        view.sync(&panel, None);
        view.state = super::LoadState::Ok;
        view.entries = (0..N)
            .map(|i| super::DirEntryInfo {
                path: path(i),
                name: name(i),
                is_dir: false,
                size: 0,
                modified: None,
                ext: "txt".into(),
                link: Default::default(),
            })
            .collect();
        Self {
            ctx: egui::Context::default(),
            panel,
            view,
            focused: false,
        }
    }

    /// 한 프레임을 그리고 이 프레임에 그린 항목 이름과 그 사각형을 돌려준다.
    /// 스크롤 애니메이션이 진행되도록 프레임마다 시각을 1/60 초씩 올린다.
    fn frame(&mut self, events: Vec<Event>) -> Vec<(String, Rect)> {
        let font = crate::settings::EffectiveFont {
            font_family: String::new(),
            font_size: 13.0,
            custom_font_path: String::new(),
            line_height: 1.0,
            font_scale_mode: String::new(),
        };
        let shortcut_chars = HashSet::new();
        let input = super::ExplorerInput {
            focused: self.focused,
            overlay_open: false,
            shortcut_chars: &shortcut_chars,
        };
        let raw = RawInput {
            time: Some(self.ctx.input(|i| i.time) + 1.0 / 60.0),
            screen_rect: Some(CELL),
            focused: true,
            events,
            ..Default::default()
        };
        let (panel, view) = (&self.panel, &mut self.view);
        let out = self.ctx.run(raw, |ctx| {
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show(ctx, |ui| {
                    super::draw_explorer(
                        ui,
                        panel,
                        view,
                        &font,
                        "virtual",
                        &[],
                        &HashSet::new(),
                        &[],
                        None,
                        &input,
                    );
                });
        });
        out.shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) if text.galley.text().starts_with("item") => {
                    Some((text.galley.text().to_owned(), text.visual_bounding_rect()))
                }
                _ => None,
            })
            .collect()
    }

    /// 스크롤 애니메이션이 끝날 만큼 프레임을 돌린다.
    fn settle(&mut self) -> Vec<(String, Rect)> {
        let mut drawn = Vec::new();
        for _ in 0..60 {
            drawn = self.frame(Vec::new());
        }
        drawn
    }
}

fn on_screen(drawn: &[(String, Rect)], i: usize) -> Option<Rect> {
    let name = name(i);
    drawn
        .iter()
        .find(|(n, r)| *n == name && CELL.contains(r.center()))
        .map(|(_, r)| *r)
}

fn click(at: Pos2) -> Vec<Event> {
    let button = |pressed| Event::PointerButton {
        pos: at,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::default(),
    };
    vec![Event::PointerMoved(at), button(true), button(false)]
}

#[test]
fn only_the_items_on_screen_are_drawn() {
    for mode in MODES {
        let mut h = Harness::new(mode);
        h.frame(Vec::new());
        let drawn = h.frame(Vec::new());
        assert!(
            on_screen(&drawn, 0).is_some(),
            "{mode:?}: the first item drawn"
        );
        assert!(
            drawn.len() < 200,
            "{mode:?}: {} of {N} items drawn",
            drawn.len()
        );
    }
}

#[test]
fn a_far_item_is_scrolled_into_view_and_a_click_selects_it() {
    for mode in MODES {
        let mut h = Harness::new(mode);
        h.frame(Vec::new());
        h.view.reveal_created(&path(15_000));
        let drawn = h.settle();
        let rect = on_screen(&drawn, 15_000)
            .unwrap_or_else(|| panic!("{mode:?}: item 15000 not on screen"));
        assert!(drawn.len() < 200, "{mode:?}: {} items drawn", drawn.len());

        h.frame(click(rect.center()));
        assert_eq!(
            h.view.selected.iter().cloned().collect::<Vec<_>>(),
            vec![path(15_000)],
            "{mode:?}"
        );
    }
}

#[test]
fn type_ahead_reaches_an_item_far_down_the_list() {
    for mode in MODES {
        let mut h = Harness::new(mode);
        h.focused = true;
        h.frame(Vec::new());
        h.frame("item18765".chars().map(|c| Event::Text(c.into())).collect());
        assert!(h.view.selected.contains(&path(18_765)), "{mode:?}");
        let drawn = h.settle();
        assert!(
            on_screen(&drawn, 18_765).is_some(),
            "{mode:?}: not on screen"
        );
    }
}

#[test]
fn the_name_input_keeps_focus_while_the_list_scrolls_away() {
    for mode in MODES {
        let mut h = Harness::new(mode);
        h.frame(Vec::new());
        h.view.start_create(PathBuf::from(DIR), true);
        h.settle();
        let editor = h.ctx.memory(|m| m.focused());
        assert!(editor.is_some(), "{mode:?}: the name input takes focus");

        h.view.reveal_created(&path(19_000));
        let drawn = h.settle();
        assert!(
            on_screen(&drawn, 19_000).is_some(),
            "{mode:?}: scrolled away"
        );
        assert!(h.view.create.is_some(), "{mode:?}: the input stays open");
        assert_eq!(
            h.ctx.memory(|m| m.focused()),
            editor,
            "{mode:?}: focus kept"
        );
    }
}

#[test]
fn the_list_keeps_one_row_per_item_down_to_the_last() {
    for mode in [ExplorerViewMode::Detail, ExplorerViewMode::List] {
        let mut h = Harness::new(mode);
        h.frame(Vec::new());
        h.view.reveal_created(&path(N - 1));
        let drawn = h.settle();
        let last = on_screen(&drawn, N - 1).expect("last item on screen");
        let before = on_screen(&drawn, N - 2).expect("the item before it on screen");
        let pitch = last.center().y - before.center().y;
        assert!(pitch > 0.0 && pitch < 40.0, "{mode:?}: pitch {pitch}");
    }
}

#[test]
fn grid_lines_far_down_start_where_the_full_grid_would_put_them() {
    let mut h = Harness::new(ExplorerViewMode::Grid);
    h.frame(Vec::new());
    let first = h.frame(Vec::new());
    // 첫 줄은 `..` 칸과 항목들이다. 항목 0 과 같은 높이에 놓인 항목 수 + 1 이 한 줄의 칸 수다.
    let top = on_screen(&first, 0).expect("item 0").center().y;
    let cols = first
        .iter()
        .filter(|(_, r)| (r.center().y - top).abs() < 1.0)
        .count()
        + 1;
    assert!(cols > 2, "cols {cols}");

    h.view.reveal_created(&path(15_000));
    let drawn = h.settle();
    let index = |n: &str| {
        n["item".len()..n.len() - ".txt".len()]
            .parse::<usize>()
            .unwrap()
    };
    let mut lines: std::collections::BTreeMap<i64, Vec<usize>> = Default::default();
    // 첫 줄은 늘 그려 화면 위 밖에 있다. 화면 안 줄만 본다.
    for (n, r) in drawn.iter().filter(|(_, r)| CELL.contains(r.center())) {
        lines
            .entry(r.center().y.round() as i64)
            .or_default()
            .push(index(n));
    }
    assert!(lines.len() > 2, "lines: {lines:?}");
    for items in lines.values() {
        let lead = items.iter().min().copied().unwrap_or_default();
        assert_eq!(
            (lead + 1) % cols,
            0,
            "a line starts at item {lead} with {cols} columns"
        );
        assert_eq!(
            items.len(),
            cols.min(N - lead),
            "line from {lead}: {items:?}"
        );
    }
}
