//! 마우스 없이 열기·이동·선택·닫기를 수행하고 비활성 행을 건너뛰는지 검사한다.
//! Esc는 소비한 뒤 트리거 포커스를 유지하고 Tab은 다음 위젯으로 전달해야 한다.

use egui::{Event, Key, Modifiers, Pos2, RawInput, Rect, vec2};
use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{MultiSelectLabels, multi_select, multi_select_popup_id};

const OPTIONS: &[&str] = &["Waiting", "Ready", "Running", "Done"];
const SALT: &str = "test_keyboard_filter";
const WIDTH: f32 = 180.0;

const LABELS: MultiSelectLabels<'static> = MultiSelectLabels {
    none: "No status",
    some: "{} selected",
    all: "All statuses",
};

fn raw(events: Vec<Event>) -> RawInput {
    RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(400.0, 400.0))),
        focused: true,
        events,
        ..Default::default()
    }
}

/// 키 한 번(누름 + 뗌).
fn key(key: Key) -> Vec<Event> {
    vec![
        Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        },
        Event::Key {
            key,
            physical_key: None,
            pressed: false,
            repeat: false,
            modifiers: Modifiers::NONE,
        },
    ]
}

/// 한 프레임 관측치.
struct Frame {
    /// 프레임이 끝난 시점에 팝업이 열려 있는지.
    open: bool,
    /// 위젯 처리 뒤 Esc가 입력 큐에 남아 있는지.
    esc_left: bool,
    /// 포커스 이동을 위해 Tab은 입력 큐에 남아 있어야 한다.
    tab_left: bool,
}

/// 한 프레임 구동.
fn frame(
    ctx: &egui::Context,
    theme: &Theme,
    selected: &mut [bool],
    disabled: Option<&[bool]>,
    events: Vec<Event>,
) -> Frame {
    let mut open = false;
    let mut esc_left = false;
    let mut tab_left = false;

    let _out = ctx.run(raw(events), |c| {
        egui::CentralPanel::default().show(c, |ui| {
            multi_select(
                ui, theme, SALT, selected, OPTIONS, disabled, &LABELS, None, WIDTH, true,
            );
            open = ui.memory(|m| m.is_popup_open(multi_select_popup_id(ui, SALT)));
            esc_left = ui.input(|i| i.key_pressed(Key::Escape));
            tab_left = ui.input(|i| i.key_pressed(Key::Tab));
        });
    });

    Frame {
        open,
        esc_left,
        tab_left,
    }
}

/// 트리거에 포커스를 준 상태까지 진행한다(빈 프레임 한 번 + `Tab` 한 번).
fn focus_trigger(ctx: &egui::Context, theme: &Theme, selected: &mut [bool]) {
    frame(ctx, theme, selected, None, Vec::new());
    frame(ctx, theme, selected, None, key(Key::Tab));
}

#[test]
fn arrow_down_opens_the_popup_from_a_focused_trigger() {
    let theme = tasty_themes::mocha_fallback();
    let ctx = egui::Context::default();
    let mut selected = vec![false; OPTIONS.len()];

    focus_trigger(&ctx, &theme, &mut selected);
    let f = frame(&ctx, &theme, &mut selected, None, key(Key::ArrowDown));
    assert!(f.open, "포커스된 트리거에서 ↓ 는 팝업을 열어야 한다");
}

#[test]
fn enter_opens_then_space_toggles_without_closing() {
    let theme = tasty_themes::mocha_fallback();
    let ctx = egui::Context::default();
    let mut selected = vec![false; OPTIONS.len()];

    focus_trigger(&ctx, &theme, &mut selected);
    let f = frame(&ctx, &theme, &mut selected, None, key(Key::Enter));
    assert!(f.open, "포커스된 트리거에서 Enter 는 팝업을 열어야 한다");
    assert_eq!(
        selected,
        vec![false; OPTIONS.len()],
        "여는 Enter 가 행을 토글해서는 안 된다"
    );

    frame(&ctx, &theme, &mut selected, None, key(Key::ArrowDown));
    let f = frame(&ctx, &theme, &mut selected, None, key(Key::Space));
    assert!(f.open, "Space 토글은 팝업을 닫지 않는다");
    assert_eq!(selected, vec![true, false, false, false]);

    frame(&ctx, &theme, &mut selected, None, key(Key::ArrowDown));
    let f = frame(&ctx, &theme, &mut selected, None, key(Key::Enter));
    assert!(f.open, "Enter 토글도 팝업을 닫지 않는다");
    assert_eq!(selected, vec![true, true, false, false]);
}

#[test]
fn arrow_up_walks_backwards_and_wraps() {
    let theme = tasty_themes::mocha_fallback();
    let ctx = egui::Context::default();
    let mut selected = vec![false; OPTIONS.len()];

    focus_trigger(&ctx, &theme, &mut selected);
    frame(&ctx, &theme, &mut selected, None, key(Key::Enter));
    frame(&ctx, &theme, &mut selected, None, key(Key::ArrowUp));
    frame(&ctx, &theme, &mut selected, None, key(Key::Space));
    assert_eq!(selected, vec![false, false, false, true]);
    frame(&ctx, &theme, &mut selected, None, key(Key::ArrowDown));
    frame(&ctx, &theme, &mut selected, None, key(Key::Space));
    assert_eq!(selected, vec![true, false, false, true]);
}

#[test]
fn active_row_skips_disabled_rows() {
    let theme = tasty_themes::mocha_fallback();
    let ctx = egui::Context::default();
    let mut selected = vec![false; OPTIONS.len()];
    // 1 번 행만 비활성 — ↓ 두 번이면 0 → 2 여야 한다(1 을 건너뛴다).
    let disabled = [false, true, false, false];

    frame(&ctx, &theme, &mut selected, Some(&disabled), Vec::new());
    frame(&ctx, &theme, &mut selected, Some(&disabled), key(Key::Tab));
    frame(
        &ctx,
        &theme,
        &mut selected,
        Some(&disabled),
        key(Key::Enter),
    );
    frame(
        &ctx,
        &theme,
        &mut selected,
        Some(&disabled),
        key(Key::ArrowDown),
    );
    frame(
        &ctx,
        &theme,
        &mut selected,
        Some(&disabled),
        key(Key::ArrowDown),
    );
    frame(
        &ctx,
        &theme,
        &mut selected,
        Some(&disabled),
        key(Key::Space),
    );
    assert_eq!(
        selected,
        vec![false, false, true, false],
        "비활성 행은 커서가 짚지 못한다"
    );
}

#[test]
fn home_and_end_jump_to_the_first_and_last_enabled_row() {
    let theme = tasty_themes::mocha_fallback();
    let ctx = egui::Context::default();
    let mut selected = vec![false; OPTIONS.len()];
    // 양 끝이 비활성 — Home/End 는 각각 1 번·2 번 행에 서야 한다.
    let disabled = [true, false, false, true];

    frame(&ctx, &theme, &mut selected, Some(&disabled), Vec::new());
    frame(&ctx, &theme, &mut selected, Some(&disabled), key(Key::Tab));
    frame(
        &ctx,
        &theme,
        &mut selected,
        Some(&disabled),
        key(Key::Enter),
    );
    frame(&ctx, &theme, &mut selected, Some(&disabled), key(Key::End));
    frame(
        &ctx,
        &theme,
        &mut selected,
        Some(&disabled),
        key(Key::Space),
    );
    assert_eq!(selected, vec![false, false, true, false]);
    frame(&ctx, &theme, &mut selected, Some(&disabled), key(Key::Home));
    frame(
        &ctx,
        &theme,
        &mut selected,
        Some(&disabled),
        key(Key::Space),
    );
    assert_eq!(selected, vec![false, true, true, false]);
}

#[test]
fn escape_closes_the_popup_keeps_focus_and_consumes_the_key() {
    let theme = tasty_themes::mocha_fallback();
    let ctx = egui::Context::default();
    let mut selected = vec![false; OPTIONS.len()];

    focus_trigger(&ctx, &theme, &mut selected);
    let f = frame(&ctx, &theme, &mut selected, None, key(Key::Enter));
    assert!(f.open);

    let f = frame(&ctx, &theme, &mut selected, None, key(Key::Escape));
    assert!(!f.open, "Esc 는 팝업을 닫는다");
    assert!(
        !f.esc_left,
        "Esc 는 소비되어야 한다 — 남으면 부모 popup 까지 함께 닫힌다"
    );

    // 포커스는 트리거에 남는다 — ↓ 만으로 곧바로 다시 열려야 한다.
    let f = frame(&ctx, &theme, &mut selected, None, key(Key::ArrowDown));
    assert!(
        f.open,
        "Esc 뒤에도 트리거가 포커스를 유지해야 ↓ 로 다시 열린다"
    );
    frame(&ctx, &theme, &mut selected, None, key(Key::Escape));

    // 닫힌 상태의 Esc 는 이 위젯 것이 아니다 — 상위 화면에 그대로 넘어가야 한다.
    let f = frame(&ctx, &theme, &mut selected, None, key(Key::Escape));
    assert!(!f.open);
    assert!(f.esc_left, "닫혀 있으면 Esc 를 가로채지 않는다");
}

#[test]
fn tab_closes_the_popup_but_leaves_the_key_for_focus_move() {
    let theme = tasty_themes::mocha_fallback();
    let ctx = egui::Context::default();
    let mut selected = vec![false; OPTIONS.len()];

    focus_trigger(&ctx, &theme, &mut selected);
    let f = frame(&ctx, &theme, &mut selected, None, key(Key::Enter));
    assert!(f.open);

    let f = frame(&ctx, &theme, &mut selected, None, key(Key::Tab));
    assert!(!f.open, "Tab 은 팝업을 닫는다");
    assert!(
        f.tab_left,
        "Tab 은 소비하지 않는다 — egui 가 이어서 포커스를 옮겨야 한다"
    );
}

#[test]
fn active_row_resets_on_each_open() {
    let theme = tasty_themes::mocha_fallback();
    let ctx = egui::Context::default();
    let mut selected = vec![false; OPTIONS.len()];

    focus_trigger(&ctx, &theme, &mut selected);
    frame(&ctx, &theme, &mut selected, None, key(Key::Enter));
    frame(&ctx, &theme, &mut selected, None, key(Key::End));
    frame(&ctx, &theme, &mut selected, None, key(Key::Escape));
    // 다시 열면 커서가 없어야 한다 — Space 만 눌러서는 아무 행도 토글되지 않는다.
    frame(&ctx, &theme, &mut selected, None, key(Key::Enter));
    frame(&ctx, &theme, &mut selected, None, key(Key::Space));
    assert_eq!(
        selected,
        vec![false; OPTIONS.len()],
        "새로 연 팝업의 커서는 초기화된다"
    );
    frame(&ctx, &theme, &mut selected, None, key(Key::ArrowDown));
    frame(&ctx, &theme, &mut selected, None, key(Key::Space));
    assert_eq!(selected, vec![true, false, false, false]);
}
