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
//! 5. 행 단위 disabled 마스크가 켜진 행은 클릭해도 선택이 바뀌지 않는다.
//! 6. 일괄 토글 행(opt-in)이 전부 켜기 / 전부 끄기를 오가고, 끄면 아예 자리를 안 쓴다.
//!
//! headless `egui::Context` 구동 패턴과 실제 `Theme` 사용은 선례
//! `path_field_focus.rs` / `table_row_click.rs` 를 그대로 따른다.

use egui::{Event, Modifiers, PointerButton, Pos2, RawInput, Rect, pos2, vec2};
use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{
    MultiSelectAllToggle, MultiSelectLabels, multi_select, multi_select_popup_id,
    multi_select_summary,
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

const ALL_TOGGLE: MultiSelectAllToggle<'static> = MultiSelectAllToggle {
    select_all: "Select all",
    clear_all: "Clear all",
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
    frame_masked(ctx, theme, selected, None, events)
}

/// [`frame`] 과 같되 행 단위 disabled 마스크를 함께 넘긴다.
fn frame_masked(
    ctx: &egui::Context,
    theme: &Theme,
    selected: &mut [bool],
    disabled: Option<&[bool]>,
    events: Vec<Event>,
) -> Frame {
    frame_full(ctx, theme, selected, disabled, None, events)
}

/// [`frame_masked`] 와 같되 일괄 토글 행을 켠다.
fn frame_all(
    ctx: &egui::Context,
    theme: &Theme,
    selected: &mut [bool],
    disabled: Option<&[bool]>,
    events: Vec<Event>,
) -> Frame {
    frame_full(ctx, theme, selected, disabled, Some(ALL_TOGGLE), events)
}

/// 모든 축을 다 받는 본체.
fn frame_full(
    ctx: &egui::Context,
    theme: &Theme,
    selected: &mut [bool],
    disabled: Option<&[bool]>,
    all_toggle: Option<MultiSelectAllToggle<'_>>,
    events: Vec<Event>,
) -> Frame {
    let mut changed = false;
    let mut trigger = Rect::NOTHING;
    let mut open = false;

    // 반환값(FullOutput)은 렌더가 없어 쓰지 않는다 — 관측은 클로저 안에서 끝난다.
    let _out = ctx.run(raw(events), |c| {
        egui::CentralPanel::default().show(c, |ui| {
            let before = ui.cursor().min;
            changed = multi_select(
                ui, theme, SALT, selected, OPTIONS, disabled, &LABELS, all_toggle, WIDTH, true,
            );
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

/// 옵션 행 한 줄의 높이 — 위젯의 checkbox 행 계산과 같은 식.
fn row_height(theme: &Theme) -> f32 {
    theme
        .checkbox_size()
        .value()
        .max(theme.font_size_body.value())
}

/// 일괄 토글 행이 켜졌을 때 옵션 목록이 아래로 밀리는 양 — 액션 행 + 간격 + 구분선
/// + 간격.
fn all_toggle_offset(theme: &Theme) -> f32 {
    let gap = theme.spacing_xs.value();
    row_height(theme) + gap + theme.border_width.value() + gap
}

/// 일괄 토글 행(메뉴 최상단)의 화면 좌표.
fn all_toggle_pos(theme: &Theme, trigger: Rect, margin: f32) -> Pos2 {
    pos2(
        trigger.left() + margin + theme.checkbox_size().value() * 0.5,
        trigger.bottom() + margin + row_height(theme) * 0.5,
    )
}

/// 일괄 토글 행이 켜진 팝업에서 `i` 번째 옵션 행의 좌표.
fn row_pos_below_all_toggle(theme: &Theme, trigger: Rect, i: usize, margin: f32) -> Pos2 {
    let p = row_pos(theme, trigger, i, margin);
    pos2(p.x, p.y + all_toggle_offset(theme))
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
fn disabled_rows_ignore_clicks_while_others_still_toggle() {
    let theme = tasty_themes::mocha_fallback();
    let ctx = egui::Context::default();
    let mut selected = vec![false, true, false, false];
    // 0번은 꺼진 채, 1번은 켜진 채 비활성 — 두 조합 모두 클릭이 먹지 않아야 한다.
    let disabled = [true, true, false, false];
    let mask = Some(&disabled[..]);

    let f = frame_masked(&ctx, &theme, &mut selected, mask, Vec::new());
    let trigger = f.trigger;
    let f = frame_masked(&ctx, &theme, &mut selected, mask, click(trigger.center()));
    assert!(
        f.open,
        "트리거는 살아 있어야 한다 — 행 disabled 는 컨트롤 disabled 가 아니다"
    );
    frame_masked(&ctx, &theme, &mut selected, mask, Vec::new());
    let margin = popup_margin(&ctx);

    let before = selected.clone();
    for i in 0..2 {
        let f = frame_masked(
            &ctx,
            &theme,
            &mut selected,
            mask,
            click(row_pos(&theme, trigger, i, margin)),
        );
        assert!(!f.changed, "{i}번째 disabled 행 클릭이 변경으로 보고됐다");
        assert_eq!(
            selected, before,
            "{i}번째 disabled 행 클릭으로 선택이 바뀌었다"
        );
        assert!(f.open, "disabled 행 클릭은 팝업 안 클릭이라 닫히면 안 된다");
    }

    // 같은 팝업의 활성 행은 기존과 똑같이 토글된다 — 마스크가 전체를 얼리지 않는다.
    let f = frame_masked(
        &ctx,
        &theme,
        &mut selected,
        mask,
        click(row_pos(&theme, trigger, 2, margin)),
    );
    assert!(f.changed, "활성 행 클릭이 변경으로 보고되지 않았다");
    assert_eq!(selected, vec![false, true, true, false]);
}

/// 마스크가 옵션보다 짧거나 `None` 이면 그 인덱스는 활성이다 — 마스크 없는 기존
/// 호출부가 조용히 전부 비활성이 되는 회귀를 막는다.
#[test]
fn a_short_mask_leaves_the_remaining_rows_enabled() {
    let theme = tasty_themes::mocha_fallback();
    let ctx = egui::Context::default();
    let mut selected = vec![false; OPTIONS.len()];
    let disabled = [true];
    let mask = Some(&disabled[..]);

    let f = frame_masked(&ctx, &theme, &mut selected, mask, Vec::new());
    let trigger = f.trigger;
    frame_masked(&ctx, &theme, &mut selected, mask, click(trigger.center()));
    frame_masked(&ctx, &theme, &mut selected, mask, Vec::new());
    let margin = popup_margin(&ctx);

    let f = frame_masked(
        &ctx,
        &theme,
        &mut selected,
        mask,
        click(row_pos(&theme, trigger, 3, margin)),
    );
    assert!(f.changed, "마스크 밖 인덱스가 비활성으로 처리됐다");
    assert!(
        selected[3],
        "마스크 밖 인덱스가 토글되지 않았다: {selected:?}"
    );
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

/// 액션 행 하나로 전부 켜고 전부 끈다 — 그 사이 팝업은 계속 살아 있어야 한다.
#[test]
fn all_toggle_selects_then_clears_every_option() {
    let theme = tasty_themes::mocha_fallback();
    let ctx = egui::Context::default();
    let mut selected = vec![false, true, false, false];

    let f = frame_all(&ctx, &theme, &mut selected, None, Vec::new());
    let trigger = f.trigger;
    let f = frame_all(&ctx, &theme, &mut selected, None, click(trigger.center()));
    assert!(f.open, "트리거 클릭으로 팝업이 열려야 한다");
    frame_all(&ctx, &theme, &mut selected, None, Vec::new());
    let margin = popup_margin(&ctx);
    let action = all_toggle_pos(&theme, trigger, margin);

    // 1) 일부만 켜진 상태 → "Select all" → 전부 켜짐.
    let f = frame_all(&ctx, &theme, &mut selected, None, click(action));
    assert!(f.changed, "일괄 선택이 변경으로 보고되지 않았다");
    assert_eq!(selected, vec![true; OPTIONS.len()], "전부 켜지지 않았다");
    assert!(
        f.open,
        "일괄 토글 클릭으로 팝업이 닫혔다 — 연속 조작 계약 위반"
    );

    // 2) 전부 켜진 상태 → "Clear all" → 전부 꺼짐.
    let f = frame_all(&ctx, &theme, &mut selected, None, click(action));
    assert!(f.changed, "일괄 해제가 변경으로 보고되지 않았다");
    assert_eq!(selected, vec![false; OPTIONS.len()], "전부 꺼지지 않았다");
    assert!(f.open, "두 번째 일괄 토글 클릭으로 팝업이 닫혔다");

    // 3) 액션 행 아래의 옵션 행은 그대로 자기 것만 토글한다.
    let f = frame_all(
        &ctx,
        &theme,
        &mut selected,
        None,
        click(row_pos_below_all_toggle(&theme, trigger, 1, margin)),
    );
    assert!(f.changed);
    assert_eq!(
        selected,
        vec![false, true, false, false],
        "액션 행 아래 옵션 행이 어긋난 위치를 잡았다: {selected:?}"
    );
}

/// 끄면(=`None`) 액션 행도 구분선도 **자리를 쓰지 않는다** — 같은 좌표가 켜면 액션
/// 행, 끄면 0 번 옵션 행이라는 대비로 고정한다(끈 쪽 렌더 회귀 방지).
#[test]
fn without_all_toggle_the_top_row_is_the_first_option() {
    let theme = tasty_themes::mocha_fallback();
    let ctx = egui::Context::default();
    let mut selected = vec![false; OPTIONS.len()];

    let f = frame(&ctx, &theme, &mut selected, Vec::new());
    let trigger = f.trigger;
    frame(&ctx, &theme, &mut selected, click(trigger.center()));
    frame(&ctx, &theme, &mut selected, Vec::new());
    let margin = popup_margin(&ctx);

    // 켰다면 액션 행이었을 좌표. 꺼져 있으므로 0 번 옵션만 켜져야 한다.
    let f = frame(
        &ctx,
        &theme,
        &mut selected,
        click(all_toggle_pos(&theme, trigger, margin)),
    );
    assert!(f.changed);
    assert_eq!(
        selected,
        vec![true, false, false, false],
        "액션 행이 없는데도 최상단이 0 번 옵션이 아니다: {selected:?}"
    );
}

/// 일괄 토글은 disabled 행을 **켤 때도 끌 때도** 건드리지 않는다. 판정도 토글 가능한
/// 행만 보므로, disabled 행이 꺼져 있어도 라벨이 "Clear all" 로 넘어가 다음 클릭이
/// 실제로 전부를 끈다(전부-선택에 영원히 묶이지 않는다).
#[test]
fn all_toggle_leaves_disabled_rows_untouched_in_both_directions() {
    let theme = tasty_themes::mocha_fallback();
    let ctx = egui::Context::default();
    // 0 번은 꺼진 채, 1 번은 켜진 채 비활성.
    let mut selected = vec![false, true, false, false];
    let disabled = [true, true, false, false];
    let mask = Some(&disabled[..]);

    let f = frame_all(&ctx, &theme, &mut selected, mask, Vec::new());
    let trigger = f.trigger;
    frame_all(&ctx, &theme, &mut selected, mask, click(trigger.center()));
    frame_all(&ctx, &theme, &mut selected, mask, Vec::new());
    let margin = popup_margin(&ctx);
    let action = all_toggle_pos(&theme, trigger, margin);

    // 1) 전부 켜기 — 활성 2·3 만 켜지고 비활성 0·1 은 그대로.
    let f = frame_all(&ctx, &theme, &mut selected, mask, click(action));
    assert!(f.changed);
    assert_eq!(
        selected,
        vec![false, true, true, true],
        "일괄 선택이 비활성 행을 건드렸다: {selected:?}"
    );

    // 2) 활성 행이 전부 켜졌으니 이제 "전부 해제" — 다시 활성 2·3 만 꺼진다.
    let f = frame_all(&ctx, &theme, &mut selected, mask, click(action));
    assert!(f.changed, "비활성 행 때문에 해제 갈래로 넘어가지 못했다");
    assert_eq!(
        selected,
        vec![false, true, false, false],
        "일괄 해제가 비활성 행을 건드렸다: {selected:?}"
    );
    assert!(f.open);
}

/// 토글 가능한 행이 하나도 없으면 액션 행은 눌러도 아무 것도 바꾸지 않는다(변경
/// 보고도 없다) — 빈 목록·전부 비활성에서 조용히 `true` 를 뱉지 않게 고정한다.
#[test]
fn all_toggle_reports_nothing_when_no_row_is_toggleable() {
    let theme = tasty_themes::mocha_fallback();
    let ctx = egui::Context::default();
    let mut selected = vec![false; OPTIONS.len()];
    let disabled = [true; 4];
    let mask = Some(&disabled[..]);

    let f = frame_all(&ctx, &theme, &mut selected, mask, Vec::new());
    let trigger = f.trigger;
    frame_all(&ctx, &theme, &mut selected, mask, click(trigger.center()));
    frame_all(&ctx, &theme, &mut selected, mask, Vec::new());
    let margin = popup_margin(&ctx);

    let f = frame_all(
        &ctx,
        &theme,
        &mut selected,
        mask,
        click(all_toggle_pos(&theme, trigger, margin)),
    );
    assert!(!f.changed, "바꿀 행이 없는데 변경으로 보고됐다");
    assert_eq!(selected, vec![false; OPTIONS.len()]);
    assert!(f.open);
}
