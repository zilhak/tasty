//! `multi_select` 상호작용 계약 테스트.
//!
//! 다중선택이 단일 `select` 와 갈리는 지점은 전부 **팝업 생존**에 걸려 있다 — 항목
//! 하나를 눌렀다고 닫히면(`CloseOnClick`) 여러 개를 켤 방법이 없다. 그래서 이 파일은
//! 렌더 픽셀이 아니라 다음 넷을 고정한다.
//!
//! 1. 항목을 연속으로 토글해도 팝업이 열린 채 유지된다.
//! 2. 팝업 바깥을 클릭하면 닫힌다.
//! 3. 선택이 바뀐 프레임에서만 `true` 를 반환한다.
//! 4. 트리거 요약 라벨이 0개 / N개 / 전부 3갈래로 갈린다.
//!
//! headless `egui::Context` 구동 패턴과 실제 `Theme` 사용은 선례
//! `path_field_focus.rs` / `table_row_click.rs` 를 그대로 따른다.

use egui::{Event, Modifiers, PointerButton, Pos2, RawInput, Rect, pos2, vec2};
use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{
    MultiSelectLabels, multi_select, multi_select_popup_id, multi_select_summary,
};

const OPTIONS: &[&str] = &["Waiting", "Ready", "Running", "Done"];
const SALT: &str = "test_status_filter";
/// 트리거 폭 — 요약 라벨이 잘리지 않을 만큼 넉넉하게.
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

/// 한 프레임 관측치.
struct Frame {
    /// 이 프레임에서 선택이 바뀌었는지(위젯 반환값).
    changed: bool,
    /// 프레임이 끝난 시점에 팝업이 열려 있는지.
    open: bool,
    /// 트리거 rect — 클릭 좌표 계산용.
    trigger: Rect,
}

/// 한 프레임 구동. 위젯을 그리고 반환값·팝업 상태·트리거 rect 를 돌려준다.
fn frame(ctx: &egui::Context, theme: &Theme, selected: &mut [bool], events: Vec<Event>) -> Frame {
    let mut changed = false;
    let mut trigger = Rect::NOTHING;
    let mut open = false;

    // 반환값(FullOutput)은 렌더가 없어 쓰지 않는다 — 관측은 클로저 안에서 끝난다.
    let _out = ctx.run(raw(events), |c| {
        egui::CentralPanel::default().show(c, |ui| {
            let before = ui.cursor().min;
            changed = multi_select(ui, theme, SALT, selected, OPTIONS, &LABELS, WIDTH, true);
            trigger = Rect::from_min_size(before, vec2(WIDTH, theme.select_height().value()));
            open = ui.memory(|m| m.is_popup_open(multi_select_popup_id(ui, SALT)));
        });
    });

    Frame {
        changed,
        open,
        trigger,
    }
}

/// 팝업 `i` 번째 행(체크박스 박스 중앙)의 화면 좌표.
///
/// 팝업은 트리거 바로 아래에 붙으므로 트리거 rect 에서 역산한다. 추정이 빗나가면
/// 체크가 켜지지 않아 테스트가 실패하므로, 좌표 가정 자체도 함께 검증된다.
fn row_pos(theme: &Theme, trigger: Rect, i: usize, margin: f32) -> Pos2 {
    let row_h = theme
        .checkbox_size()
        .value()
        .max(theme.font_size_body.value());
    let gap = theme.spacing_xs.value();
    pos2(
        trigger.left() + margin + theme.checkbox_size().value() * 0.5,
        trigger.bottom() + margin + (row_h + gap) * i as f32 + row_h * 0.5,
    )
}

/// 팝업 프레임의 inner margin — egui 기본 `Frame::popup` 값.
fn popup_margin(ctx: &egui::Context) -> f32 {
    ctx.style().spacing.menu_margin.left as f32
}

#[test]
fn consecutive_item_toggles_keep_the_popup_open() {
    let theme = tasty_themes::mocha_fallback();
    let ctx = egui::Context::default();
    let mut selected = vec![false; OPTIONS.len()];

    // 1) 트리거 클릭 → 팝업 열림.
    let f = frame(&ctx, &theme, &mut selected, Vec::new());
    let trigger = f.trigger;
    let f = frame(&ctx, &theme, &mut selected, click(trigger.center()));
    assert!(f.open, "트리거 클릭으로 팝업이 열려야 한다");

    // 팝업이 실제로 배치되도록 한 프레임 더.
    frame(&ctx, &theme, &mut selected, Vec::new());
    let margin = popup_margin(&ctx);

    // 2) 행 3개를 연속 클릭 — 매번 팝업이 살아 있어야 한다.
    for i in 0..3 {
        let f = frame(
            &ctx,
            &theme,
            &mut selected,
            click(row_pos(&theme, trigger, i, margin)),
        );
        assert!(
            f.open,
            "{i}번째 항목을 누른 뒤 팝업이 닫혔다 — CloseOnClick 회귀"
        );
        assert!(f.changed, "{i}번째 항목 클릭이 변경으로 보고되지 않았다");
        assert!(selected[i], "{i}번째 항목이 켜지지 않았다: {selected:?}");
    }
    assert_eq!(
        selected,
        vec![true, true, true, false],
        "연속 토글 결과가 누적되지 않았다"
    );
}

#[test]
fn clicking_outside_closes_the_popup() {
    let theme = tasty_themes::mocha_fallback();
    let ctx = egui::Context::default();
    let mut selected = vec![false; OPTIONS.len()];

    let f = frame(&ctx, &theme, &mut selected, Vec::new());
    let trigger = f.trigger;
    let f = frame(&ctx, &theme, &mut selected, click(trigger.center()));
    assert!(f.open);
    frame(&ctx, &theme, &mut selected, Vec::new());

    // 팝업/트리거에서 충분히 떨어진 화면 우하단.
    let f = frame(&ctx, &theme, &mut selected, click(pos2(360.0, 360.0)));
    assert!(!f.open, "팝업 바깥 클릭으로 닫히지 않았다");
    assert!(!f.changed, "바깥 클릭이 선택 변경으로 보고됐다");
}

#[test]
fn unchanged_frames_report_false() {
    let theme = tasty_themes::mocha_fallback();
    let ctx = egui::Context::default();
    let mut selected = vec![false; OPTIONS.len()];

    // 아무 입력 없는 프레임 · 트리거를 여는 프레임 — 둘 다 선택 변경이 아니다.
    let f = frame(&ctx, &theme, &mut selected, Vec::new());
    assert!(!f.changed);
    let trigger = f.trigger;
    let f = frame(&ctx, &theme, &mut selected, click(trigger.center()));
    assert!(f.open);
    assert!(!f.changed, "여는 클릭이 선택 변경으로 보고됐다");
}

#[test]
fn summary_label_has_three_branches() {
    // 0개 — 옵션이 남아 있어도 "없음".
    assert_eq!(
        multi_select_summary(&LABELS, &[false, false, false, false]),
        "No status"
    );
    // 일부 — `{}` 가 개수로 치환된다.
    assert_eq!(
        multi_select_summary(&LABELS, &[true, false, true, false]),
        "2 selected"
    );
    assert_eq!(
        multi_select_summary(&LABELS, &[true, false, false, false]),
        "1 selected"
    );
    // 전부 — 개수 대신 별도 문구.
    assert_eq!(
        multi_select_summary(&LABELS, &[true, true, true, true]),
        "All statuses"
    );
    // 옵션이 0개면 "전부" 가 아니라 "없음" 이다 — 빈 목록에 "전부" 는 오해를 준다.
    assert_eq!(multi_select_summary(&LABELS, &[]), "No status");
}

#[test]
fn summary_replaces_only_the_first_placeholder() {
    // 호출자가 실수로 `{}` 를 두 번 넣어도 개수는 한 번만 들어간다(뒷쪽은 원문 유지).
    let labels = MultiSelectLabels {
        none: "none",
        some: "{} of {}",
        all: "all",
    };
    assert_eq!(multi_select_summary(&labels, &[true, false]), "1 of {}");
}
