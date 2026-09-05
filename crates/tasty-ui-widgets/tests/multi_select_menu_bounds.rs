//! `multi_select` 팝업 메뉴의 크기 제약 계약 테스트.
//!
//! 디자인 `components/forms/MultiSelect` 가 확정한 두 제약을 고정한다.
//!
//! 1. 옵션이 많아도 메뉴가 세로로 무한정 늘어나지 않는다 —
//!    `multiselect_menu_max_height`(= `autocomplete_max_height`, 220) 에서 멈추고
//!    내부 스크롤로 넘어간다.
//! 2. 라벨이 길어도 메뉴가 가로로 무한정 늘어나지 않는다 —
//!    `multiselect_menu_max_width`(320) 에서 멈추고 행 라벨이 말줄임된다.
//!
//! 짧은 목록(기존 DAG 상태 필터 모양)은 두 제약 어디에도 닿지 않아 **크기가 그대로**
//! 라는 것도 함께 고정한다 — 제약 도입이 기존 소비자를 건드리지 않았다는 회귀 가드다.
//!
//! headless `egui::Context` 구동 패턴은 선례 `multi_select_toggle.rs` 를 따른다.

use egui::{Event, Modifiers, PointerButton, Pos2, RawInput, Rect, vec2};
use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{MultiSelectLabels, multi_select, multi_select_popup_id};

/// 트리거 폭 — max-width(320) 보다 좁아야 "내용이 밀어 올린 폭" 을 관측할 수 있다.
const WIDTH: f32 = 160.0;

const LABELS: MultiSelectLabels<'static> = MultiSelectLabels {
    none: "No status",
    some: "{} selected",
    all: "All statuses",
};

/// 한 줄이 320px 을 확실히 넘는 라벨 20 종 — max-height·max-width 를 동시에 자극한다.
const LONG_OPTIONS: &[&str] = &[
    "Very long option label number 01 that overflows the menu max width",
    "Very long option label number 02 that overflows the menu max width",
    "Very long option label number 03 that overflows the menu max width",
    "Very long option label number 04 that overflows the menu max width",
    "Very long option label number 05 that overflows the menu max width",
    "Very long option label number 06 that overflows the menu max width",
    "Very long option label number 07 that overflows the menu max width",
    "Very long option label number 08 that overflows the menu max width",
    "Very long option label number 09 that overflows the menu max width",
    "Very long option label number 10 that overflows the menu max width",
    "Very long option label number 11 that overflows the menu max width",
    "Very long option label number 12 that overflows the menu max width",
    "Very long option label number 13 that overflows the menu max width",
    "Very long option label number 14 that overflows the menu max width",
    "Very long option label number 15 that overflows the menu max width",
    "Very long option label number 16 that overflows the menu max width",
    "Very long option label number 17 that overflows the menu max width",
    "Very long option label number 18 that overflows the menu max width",
    "Very long option label number 19 that overflows the menu max width",
    "Very long option label number 20 that overflows the menu max width",
];

/// 기존 소비자(DAG 상태 필터) 모양 — 짧은 라벨 6 종.
const SHORT_OPTIONS: &[&str] = &["Waiting", "Ready", "Running", "Done", "Failed", "Canceled"];

fn raw(events: Vec<Event>) -> RawInput {
    RawInput {
        // 메뉴가 화면에 눌려 좁아지지 않도록 넉넉한 화면.
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1200.0, 1200.0))),
        focused: true,
        events,
        ..Default::default()
    }
}

fn click(p: Pos2) -> Vec<Event> {
    vec![
        Event::PointerMoved(p),
        Event::PointerButton {
            pos: p,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::default(),
        },
        Event::PointerButton {
            pos: p,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::default(),
        },
    ]
}

/// 팝업을 연 뒤 안정된 프레임에서 메뉴(Area) rect 를 돌려준다.
fn open_and_measure(theme: &Theme, salt: &str, options: &[&str]) -> Rect {
    let ctx = egui::Context::default();
    let mut selected = vec![false; options.len()];
    let mut popup_id = None;
    let mut trigger = Rect::NOTHING;

    let frame = |events: Vec<Event>,
                 selected: &mut Vec<bool>,
                 trigger: &mut Rect,
                 popup_id: &mut Option<egui::Id>| {
        let _out = ctx.run(raw(events), |c| {
            egui::CentralPanel::default().show(c, |ui| {
                let before = ui.cursor().min;
                multi_select(
                    ui, theme, salt, selected, options, None, &LABELS, None, WIDTH, true,
                );
                *trigger = Rect::from_min_size(before, vec2(WIDTH, theme.select_height().value()));
                *popup_id = Some(multi_select_popup_id(ui, salt));
            });
        });
    };

    frame(Vec::new(), &mut selected, &mut trigger, &mut popup_id);
    let center = trigger.center();
    frame(click(center), &mut selected, &mut trigger, &mut popup_id);
    // 팝업이 실제로 배치·측정되도록 두 프레임 더 (첫 프레임의 크기는 추정치다).
    frame(Vec::new(), &mut selected, &mut trigger, &mut popup_id);
    frame(Vec::new(), &mut selected, &mut trigger, &mut popup_id);

    let id = popup_id.expect("팝업 id 가 관측되지 않았다");
    ctx.memory(|m| m.area_rect(id))
        .expect("팝업 Area 가 배치되지 않았다 — 열리지 않았을 가능성")
}

/// 팝업 프레임이 본문 바깥에 더하는 가로 여유 — 위젯이 쓰는 **그 함수를 부른다.**
///
/// 예전에는 같은 산술을 여기에 다시 적고 "위젯 쪽 `popup_chrome_width` 와 같은 계산"
/// 이라고 주석에 적었는데, 지키는 것이 없었다. 게다가 그 사본은 `Style::default()` 를
/// 쓰고 위젯은 `ui.style()` 을 써서 **산술이 같아도 입력이 달랐다** — 두 값이 우연히
/// 같을 때만 참인 문장이었다.
///
/// ★ 다만 이 값은 아래 단정들에서 **지지항이 아니다**(실측). 상한에 더하는 여유로만
/// 쓰이고, 계수를 0·1·4 로 흔들어도 이 파일의 어떤 단정도 안 죽는다 — 실측치가 상한에서
/// 그만큼 떨어져 있기 때문이다. 그러니 이 함수를 고쳐도 초록인 것은 **덮였다는 뜻이
/// 아니다.** 여기 있는 이유는 상한을 "본문 상한 + 프레임 여유" 로 **읽히게** 쓰기
/// 위해서이지, 그 여유를 검사하기 위해서가 아니다.
fn chrome() -> f32 {
    tasty_ui_widgets::popup_chrome_width(&egui::Style::default())
}

#[test]
fn long_labels_clamp_the_menu_width() {
    let theme = tasty_themes::mocha_fallback();
    let rect = open_and_measure(&theme, "bounds_long", LONG_OPTIONS);

    // max-width 는 메뉴 **상자 전체**의 상한이다 — 프레임 여유까지 포함해 320 이하.
    let limit = theme.multiselect_menu_max_width().value();
    assert!(
        rect.width() <= limit,
        "긴 라벨이 메뉴 폭을 max-width 밖으로 밀었다: {} > {limit}",
        rect.width()
    );
    // 반대 방향도 고정 — 클램프가 트리거 폭까지 쪼그라뜨리면 라벨이 전부 "…" 가 된다.
    assert!(
        rect.width() > WIDTH,
        "메뉴가 내용에 맞춰 넓어지지 않았다: {} <= {WIDTH}",
        rect.width()
    );
}

#[test]
fn many_options_clamp_the_menu_height() {
    let theme = tasty_themes::mocha_fallback();
    let rect = open_and_measure(&theme, "bounds_many", LONG_OPTIONS);

    // 세로는 ScrollArea 의 max_height 가 **본문** 상한이라 프레임 여유가 더 붙는다.
    let limit = theme.multiselect_menu_max_height().value() + chrome();
    assert!(
        rect.height() <= limit,
        "옵션 {}개가 메뉴 높이를 max-height 밖으로 밀었다: {} > {limit}",
        LONG_OPTIONS.len(),
        rect.height()
    );
}

#[test]
fn short_option_lists_are_untouched_by_the_clamps() {
    let theme = tasty_themes::mocha_fallback();
    let rect = open_and_measure(&theme, "bounds_short", SHORT_OPTIONS);

    // 폭은 트리거 폭(min-width) 그대로 — 짧은 라벨은 이보다 좁다.
    assert!(
        rect.width() <= WIDTH + chrome(),
        "짧은 라벨 목록의 메뉴가 트리거보다 넓어졌다: {}",
        rect.width()
    );
    // 높이는 max-height 근처에도 못 간다(6행 × ~20px).
    assert!(
        rect.height() < theme.multiselect_menu_max_height().value(),
        "6행짜리 메뉴가 max-height 에 닿았다: {}",
        rect.height()
    );
}

/// `multiselect_menu_max_height` 는 AutoComplete 드롭다운과 **같은 값**을 공유한다
/// (디자인 판정 — 값 신설 없이 재사용). 한쪽만 바뀌면 여기서 걸린다.
#[test]
fn menu_max_height_reuses_the_autocomplete_value() {
    let theme = tasty_themes::mocha_fallback();
    assert_eq!(
        theme.multiselect_menu_max_height(),
        theme.autocomplete_max_height()
    );
}
