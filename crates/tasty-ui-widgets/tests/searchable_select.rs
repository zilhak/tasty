//! SearchableSelect 를 실제 egui 프레임으로 구동한다. 트리거 높이, 검색칸 포커스와 클릭, 필터와 고정 행,
//! 결과 없음 행, 키보드 확정, 닫을 때 검색어 초기화, 숨은 값과 목록에 없는 값의 트리거 문구를 확인한다.

// 이유: 테스트에서는 사용하지 않는 반환값을 버리는 것을 허용한다.
#![allow(clippy::let_underscore_must_use)]
use egui::{Event, Key, Modifiers, PointerButton, Pos2, RawInput, Rect, pos2, vec2};
use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{
    SearchableSelect, SearchableSelectLabels, SearchableSelectOption, searchable_select_filter,
    searchable_select_popup_id, searchable_select_search_id,
};

const SALT: &str = "t_search_select";
const WIDTH: f32 = 200.0;
/// 포인터를 둘 빈 자리. 트리거와 팝업 어디에도 걸리지 않는다.
const AWAY: Pos2 = pos2(560.0, 580.0);

const OPTIONS: [SearchableSelectOption<'static>; 5] = [
    SearchableSelectOption::new("JetBrains Mono"),
    SearchableSelectOption::new("Fira Code"),
    SearchableSelectOption::new("D2Coding"),
    SearchableSelectOption::new("Ubuntu Mono"),
    SearchableSelectOption::new("Menlo"),
];
const PINNED: [SearchableSelectOption<'static>; 1] = [SearchableSelectOption::with_value(
    "",
    "Default (monospace)",
)];
const LABELS: SearchableSelectLabels<'static> = SearchableSelectLabels {
    placeholder: "Choose a font",
    search_placeholder: "Search fonts",
    empty: "No matching fonts",
    loading: "Loading fonts",
};

fn theme() -> Theme {
    Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0)
}

fn key(k: Key) -> Vec<Event> {
    [true, false]
        .into_iter()
        .map(|pressed| Event::Key {
            key: k,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: Modifiers::NONE,
        })
        .collect()
}

fn click(at: Pos2) -> Vec<Event> {
    let press = |pressed| Event::PointerButton {
        pos: at,
        button: PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    vec![Event::PointerMoved(at), press(true), press(false)]
}

/// 한 프레임 관측치.
struct Frame {
    changed: bool,
    open: bool,
    search_focused: bool,
    trigger: Rect,
    /// 그려진 글자와 그 왼쪽 위 위치.
    texts: Vec<(String, Pos2)>,
}

impl Frame {
    fn has(&self, s: &str) -> bool {
        self.texts.iter().any(|(t, _)| t == s)
    }

    /// 글자 한 줄의 가운데쯤. 그 행을 누를 때 쓴다.
    fn at(&self, s: &str) -> Pos2 {
        let (_, p) = self
            .texts
            .iter()
            .find(|(t, _)| t == s)
            .unwrap_or_else(|| panic!("{s:?} 를 그리지 않았다: {:?}", self.texts));
        *p + vec2(4.0, 6.0)
    }

    /// 열린 목록에 보이는 옵션 문구(고정 행 포함). 트리거 문구는 트리거 rect 안이라 뺀다.
    fn listed(&self, all: &[&str]) -> Vec<String> {
        self.texts
            .iter()
            .filter(|(t, p)| all.contains(&t.as_str()) && !self.trigger.contains(*p))
            .map(|(t, _)| t.clone())
            .collect()
    }
}

struct Rig {
    ctx: egui::Context,
    theme: Theme,
    value: Option<String>,
    enabled: bool,
}

impl Rig {
    fn new(value: Option<&str>) -> Self {
        let ctx = egui::Context::default();
        let mut rig = Self {
            ctx,
            theme: theme(),
            value: value.map(str::to_owned),
            enabled: true,
        };
        // 첫 프레임은 폰트 준비용이다.
        rig.frame(vec![]);
        rig
    }

    fn frame(&mut self, events: Vec<Event>) -> Frame {
        let mut changed = false;
        let mut open = false;
        let mut search_focused = false;
        let mut trigger = Rect::NOTHING;
        let (theme, value, enabled) = (&self.theme, &mut self.value, self.enabled);
        let out = self.ctx.run(
            RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(600.0, 600.0))),
                focused: true,
                events,
                ..Default::default()
            },
            |c| {
                egui::CentralPanel::default().show(c, |ui| {
                    let origin = ui.cursor().min;
                    changed = SearchableSelect::new(SALT, &OPTIONS, LABELS)
                        .pinned(&PINNED)
                        .width(LogicalPx(WIDTH))
                        .enabled(enabled)
                        .show(ui, theme, value);
                    trigger =
                        Rect::from_min_size(origin, vec2(WIDTH, theme.select_height().value()));
                    let id = searchable_select_popup_id(ui, SALT);
                    open = ui.memory(|m| m.is_popup_open(id));
                    search_focused = ui.memory(|m| m.has_focus(searchable_select_search_id(id)));
                });
            },
        );
        let texts = out
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Text(t) => Some((t.galley.text().to_owned(), t.pos)),
                _ => None,
            })
            .collect();
        Frame {
            changed,
            open,
            search_focused,
            trigger,
            texts,
        }
    }

    /// 트리거를 눌러 연다. 포인터는 다음 프레임에 빈 자리로 뺀다.
    fn open(&mut self) -> Frame {
        let f = self.frame(click(pos2(60.0, 20.0)));
        assert!(f.open, "트리거를 눌렀는데 열리지 않았다");
        self.frame(vec![Event::PointerMoved(AWAY)])
    }
}

fn all_labels() -> Vec<&'static str> {
    PINNED.iter().chain(&OPTIONS).map(|o| o.label).collect()
}

#[test]
fn the_trigger_is_select_height_and_caller_width() {
    let ctx = egui::Context::default();
    let mut rect = Rect::NOTHING;
    let th = theme();
    // 프레임 출력은 쓰지 않고 트리거가 차지한 rect 만 읽는다.
    let _ = ctx.run(RawInput::default(), |c| {
        egui::CentralPanel::default().show(c, |ui| {
            let mut v = Some("Fira Code".to_owned());
            rect = ui
                .scope(|ui| {
                    SearchableSelect::new("h", &OPTIONS, LABELS)
                        .width(LogicalPx(WIDTH))
                        .show(ui, &th, &mut v)
                })
                .response
                .rect;
        });
    });
    assert_eq!(rect.height(), th.select_height().value());
    assert_eq!(rect.width(), WIDTH);
}

#[test]
fn opening_focuses_the_search_field_and_clicking_it_keeps_the_list_open() {
    let mut rig = Rig::new(Some("Fira Code"));
    let f = rig.open();
    assert!(f.search_focused, "열린 직후 검색칸이 포커스를 가져야 한다");
    let field = f.at("Search fonts");
    let f = rig.frame(click(field));
    assert!(f.open, "검색칸 클릭으로 목록이 닫혔다");
    let f = rig.frame(vec![Event::PointerMoved(AWAY)]);
    assert!(f.open && f.search_focused);
}

#[test]
fn typing_filters_case_insensitively_and_keeps_the_pinned_row() {
    let mut rig = Rig::new(Some("Fira Code"));
    rig.open();
    let f = rig.frame(vec![Event::Text("mon".into())]);
    let f2 = rig.frame(vec![]);
    let listed = f2.listed(&all_labels());
    assert_eq!(
        listed,
        vec!["Default (monospace)", "JetBrains Mono", "Ubuntu Mono"],
        "첫 프레임: {:?}",
        f.texts
    );
    // 숨은 현재 값의 문구는 트리거에 그대로 있다.
    assert!(
        f2.texts
            .iter()
            .any(|(t, p)| t == "Fira Code" && f2.trigger.contains(*p))
    );
    assert!(!f2.changed);
}

#[test]
fn the_empty_row_shows_on_no_match_and_clicking_it_changes_nothing() {
    let mut rig = Rig::new(Some("Fira Code"));
    rig.open();
    rig.frame(vec![Event::Text("zzz".into())]);
    let f = rig.frame(vec![]);
    assert!(f.has("No matching fonts"));
    assert_eq!(f.listed(&all_labels()), vec!["Default (monospace)"]);
    let empty = f.at("No matching fonts");
    let f = rig.frame(click(empty));
    assert!(!f.changed);
    assert!(f.open, "결과 없음 행을 눌러 목록이 닫혔다");
    assert_eq!(rig.value.as_deref(), Some("Fira Code"));
}

#[test]
fn down_then_enter_commits_closes_and_the_next_open_has_an_empty_query() {
    let mut rig = Rig::new(Some("Fira Code"));
    rig.open();
    rig.frame(vec![Event::Text("CO".into())]);
    // 입력 직후 활성 행은 첫 일치 행(Fira Code), ↓ 로 다음 일치 행(D2Coding).
    let f = rig.frame(key(Key::ArrowDown));
    assert!(f.open);
    let f = rig.frame(key(Key::Enter));
    assert!(f.changed, "Enter 확정이 true 를 돌려야 한다");
    assert!(!f.open, "Enter 뒤 목록이 닫혀야 한다");
    assert_eq!(rig.value.as_deref(), Some("D2Coding"));

    let f = rig.open();
    assert!(
        f.has("Search fonts"),
        "다시 열면 검색어가 비어 placeholder 가 보여야 한다"
    );
    assert!(!f.has("CO"));
    assert_eq!(f.listed(&all_labels()).len(), all_labels().len());
}

#[test]
fn opening_starts_the_cursor_on_the_current_value() {
    let mut rig = Rig::new(Some("D2Coding"));
    rig.open();
    // 활성 행이 D2Coding 이므로 ↓ 한 번이면 Ubuntu Mono 다.
    rig.frame(key(Key::ArrowDown));
    let f = rig.frame(key(Key::Enter));
    assert!(f.changed);
    assert_eq!(rig.value.as_deref(), Some("Ubuntu Mono"));
}

#[test]
fn the_cursor_stops_at_the_list_ends() {
    let mut rig = Rig::new(Some("Menlo"));
    rig.open();
    rig.frame(key(Key::ArrowDown));
    let f = rig.frame(key(Key::Enter));
    assert!(!f.changed, "끝에서 ↓ 는 순환하지 않고 Menlo 에 머문다");
    assert_eq!(rig.value.as_deref(), Some("Menlo"));
    assert!(!f.open, "같은 값이라도 Enter 는 닫는다");

    let mut rig = Rig::new(Some("JetBrains Mono"));
    rig.open();
    for _ in 0..3 {
        rig.frame(key(Key::ArrowUp));
    }
    let f = rig.frame(key(Key::Enter));
    assert!(f.changed);
    assert_eq!(
        rig.value.as_deref(),
        Some(""),
        "↑ 는 고정 행까지 가고 멈춘다"
    );
}

#[test]
fn esc_closes_and_returns_focus_to_the_trigger() {
    let mut rig = Rig::new(Some("Fira Code"));
    rig.open();
    rig.frame(vec![Event::Text("x".into())]);
    let f = rig.frame(key(Key::Escape));
    assert!(!f.open);
    assert!(!f.changed);
    // 트리거가 포커스를 가지면 ↓ 로 다시 열 수 있다.
    let f = rig.frame(key(Key::ArrowDown));
    assert!(f.open, "Esc 뒤 트리거로 포커스가 돌아오지 않았다");
    let f = rig.frame(vec![]);
    assert!(
        f.has("Search fonts"),
        "Esc 로 닫으면 검색어가 지워져야 한다"
    );
}

#[test]
fn tab_closes_the_list() {
    let mut rig = Rig::new(Some("Fira Code"));
    rig.open();
    let f = rig.frame(key(Key::Tab));
    assert!(!f.open);
}

#[test]
fn clicking_an_option_commits_and_closes() {
    let mut rig = Rig::new(None);
    let f = rig.open();
    let row = f.at("Menlo");
    let f = rig.frame(click(row));
    assert!(f.changed);
    assert!(!f.open);
    assert_eq!(rig.value.as_deref(), Some("Menlo"));
}

#[test]
fn the_trigger_shows_placeholder_pinned_label_and_unknown_values() {
    let f = Rig::new(None).frame(vec![]);
    assert!(f.has("Choose a font"));
    let f = Rig::new(Some("")).frame(vec![]);
    assert!(f.has("Default (monospace)"));
    let mut rig = Rig::new(Some("Not Installed Font"));
    let f = rig.frame(vec![]);
    assert!(
        f.has("Not Installed Font"),
        "목록에 없는 값도 트리거에 보여야 한다"
    );
    assert_eq!(rig.value.as_deref(), Some("Not Installed Font"));
}

#[test]
fn a_disabled_select_does_not_open() {
    let mut rig = Rig::new(Some("Fira Code"));
    rig.enabled = false;
    let f = rig.frame(click(pos2(60.0, 20.0)));
    assert!(!f.open);
    let f = rig.frame(vec![]);
    assert!(!f.open);
}

#[test]
fn the_filter_trims_and_ignores_case() {
    assert_eq!(searchable_select_filter(&OPTIONS, "  MONO "), vec![0, 3]);
    assert_eq!(searchable_select_filter(&OPTIONS, ""), vec![0, 1, 2, 3, 4]);
    assert!(searchable_select_filter(&OPTIONS, "zzz").is_empty());
}
