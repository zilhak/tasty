//! `PathField` 편집모드(editing)와 viewport focus 의 분리 회귀 테스트.
//!
//! 재현 대상 버그(markdown 주소창 진동): egui `has_focus()` 는 viewport focused
//! (`RawInput.focused`)에 게이트돼 있어, plugin SDK `repaint_last` 의 focused=false
//! 재-run 프레임마다 editing 이 false 로 떨어지고, 다음 실제 프레임이 "편집 진입" 을
//! 재감지해 buffer clear + 재-paint 를 무한 재점화했다 — placeholder ↔ 편집필드가
//! 프레임마다 교대하는 진동. editing 판정을 egui memory 포커스 기반으로 분리해 끊는다.
//!
//! markdown plugin 의 `paint()` 루프(버퍼 sync → draw → 편집 진입 시 recent fetch +
//! buffer clear + focused=false 재-run)를 headless 로 미러링한다.

use egui::{Event, Modifiers, PointerButton, Pos2, RawInput, Rect, pos2, vec2};
use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{Input, PathField, PathFieldOutcome};

const FILE_PATH: &str = "E:/docs/readme.md";

/// markdown plugin 의 주소창 상태 미러 + 계약 검증용 카운터.
struct AddrState {
    buffer: String,
    editing: bool,
    active: Option<usize>,
    recent: Vec<String>,
    /// host `recent.query` 호출 미러 — "편집 진입당 1회" 계약 검증.
    fetch_recent_calls: usize,
    /// 편집 진입 전이(false→true) 감지 횟수 — 클릭 1회당 1회여야 한다.
    entry_transitions: usize,
}

impl AddrState {
    fn new() -> Self {
        Self {
            buffer: String::new(),
            editing: false,
            active: None,
            recent: Vec::new(),
            fetch_recent_calls: 0,
            entry_transitions: 0,
        }
    }
}

/// markdown 주소창 draw 미러 — 상단 40px 바 안의 PathField.
fn draw(ctx: &egui::Context, theme: &Theme, addr: &mut AddrState) {
    let bar_frame = egui::Frame::new()
        .fill(theme.bg_sidebar().to_egui())
        .inner_margin(egui::Margin::symmetric(theme.spacing_sm.value() as i8, 0));
    egui::TopBottomPanel::top("md_addr_bar")
        .exact_height(40.0)
        .frame(bar_frame)
        .resizable(false)
        .show_separator_line(false)
        .show(ctx, |ui| {
            let entries: Vec<&str> = addr.recent.iter().map(String::as_str).collect();
            ui.horizontal_centered(|ui| {
                let outcome = PathField::new("md_addr")
                    .placeholder("Path to markdown file")
                    .empty_label("No recent files")
                    .width(ui.available_width())
                    .show(
                        ui,
                        theme,
                        &mut addr.buffer,
                        &mut addr.editing,
                        &mut addr.active,
                        &entries,
                        FILE_PATH,
                    );
                assert_eq!(
                    outcome,
                    PathFieldOutcome::None,
                    "이 시나리오엔 확정/원복이 없어야 한다"
                );
            });
        });
    egui::CentralPanel::default().show(ctx, |ui| {
        ui.label("body");
    });
}

/// host build_raw_input 미러: screen_rect + focused + events.
fn raw(focused: bool, events: Vec<Event>) -> RawInput {
    RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))),
        focused,
        events,
        ..Default::default()
    }
}

fn ptr_move(x: f32, y: f32) -> Event {
    Event::PointerMoved(pos2(x, y))
}

fn ptr_btn(x: f32, y: f32, pressed: bool) -> Event {
    Event::PointerButton {
        pos: pos2(x, y),
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::default(),
    }
}

// 필드 위치: 상단 바 40px 안, x=200 은 필드 내부.
const FX: f32 = 200.0;
const FY: f32 = 20.0;

/// markdown plugin `paint()` 미러 — set_context 1회 (버퍼 sync + draw + 조건부
/// entry repaint). SDK 수정으로 entry repaint 는 직전 focused(=true)를 보존하지만,
/// 여기서는 **미수정 SDK 의 최악 조건(focused=false 재-run)** 을 그대로 재현해
/// PathField 수정 단독으로도 루프가 끊기는지 검증한다.
fn paint(ctx: &egui::Context, theme: &Theme, addr: &mut AddrState, input: RawInput) {
    if !addr.editing {
        addr.buffer = FILE_PATH.to_string();
    }
    let prev_editing = addr.editing;
    let _out = ctx.run(input, |c| draw(c, theme, addr));

    if addr.editing && !prev_editing {
        addr.entry_transitions += 1;
        // fetch_recent + clear-on-focus + repaint(빈 events, focused=false).
        addr.fetch_recent_calls += 1;
        addr.recent = vec![
            "E:/docs/readme.md".into(),
            "E:/docs/design.md".into(),
            "E:/notes/todo.md".into(),
        ];
        addr.buffer.clear();
        let _out = ctx.run(raw(false, vec![]), |c| draw(c, theme, addr));
    }
}

/// ① 클릭으로 편집 진입 → ② focused=false + 빈 events 재-run 에서도 editing 유지.
/// (memory 포커스는 빈 events 재-run 에서 지워지지 않는다 — viewport 게이트 분리 검증.)
#[test]
fn editing_survives_unfocused_rerun() {
    let theme = tasty_themes::mocha_fallback();
    let ctx = egui::Context::default();
    let mut addr = AddrState::new();

    // hover 이동 프레임 — 아직 비편집.
    let _out = ctx.run(raw(true, vec![ptr_move(FX, FY)]), |c| {
        draw(c, &theme, &mut addr)
    });
    assert!(!addr.editing, "클릭 전엔 편집모드가 아니다");

    // 클릭 press → 편집 진입.
    let _out = ctx.run(raw(true, vec![ptr_btn(FX, FY, true)]), |c| {
        draw(c, &theme, &mut addr)
    });
    assert!(addr.editing, "클릭 press 프레임에 편집모드 진입");

    // focused=false + 빈 events 재-run (SDK repaint_last 의 미수정 최악 조건 /
    // 실제 surface blur 와 동형) → editing 은 memory 포커스 기반이라 유지된다.
    let _out = ctx.run(raw(false, vec![]), |c| draw(c, &theme, &mut addr));
    assert!(
        addr.editing,
        "focused=false 재-run 프레임에서 editing 이 떨어지면 진동 루프가 재점화된다"
    );
}

/// ③ markdown paint 루프 미러를 클릭 후 12 프레임(마우스 jitter 8 + 무입력 4) 돌려
/// 편집 진입 전이가 클릭 1회뿐이고 buffer 재클리어·editing 진동이 없음을 단언한다.
/// (recent.query "편집 진입당 1회" 계약 — main.rs 의 fetch_recent — 도 함께 고정.)
#[test]
fn entry_repaint_does_not_reignite_flicker_loop() {
    let theme = tasty_themes::mocha_fallback();
    let ctx = egui::Context::default();
    let mut addr = AddrState::new();

    // idle hover 2 프레임.
    paint(&ctx, &theme, &mut addr, raw(true, vec![ptr_move(FX, FY)]));
    paint(&ctx, &theme, &mut addr, raw(true, vec![ptr_move(FX, FY)]));
    assert!(!addr.editing);
    assert_eq!(addr.entry_transitions, 0);

    // 클릭: press → release. press 프레임에 진입 + entry repaint 1회.
    paint(
        &ctx,
        &theme,
        &mut addr,
        raw(true, vec![ptr_btn(FX, FY, true)]),
    );
    assert!(addr.editing, "press 프레임에 편집 진입");
    assert_eq!(addr.entry_transitions, 1, "진입 전이는 클릭 프레임 1회");
    assert_eq!(addr.buffer, "", "진입 시 버퍼는 clear 된 채 유지");
    paint(
        &ctx,
        &theme,
        &mut addr,
        raw(true, vec![ptr_btn(FX, FY, false)]),
    );

    // 이후: 미세 jitter 이동 8 프레임 + 무입력 재-forward 4 프레임.
    for i in 0..8 {
        let dx = (i % 2) as f32; // 1px jitter
        paint(
            &ctx,
            &theme,
            &mut addr,
            raw(true, vec![ptr_move(FX + dx, FY)]),
        );
        assert!(addr.editing, "jitter-{i}: editing 진동 없음");
        assert_eq!(
            addr.buffer, "",
            "jitter-{i}: 버퍼가 경로로 재동기화되지 않음"
        );
    }
    for i in 0..4 {
        paint(&ctx, &theme, &mut addr, raw(true, vec![]));
        assert!(addr.editing, "empty-{i}: editing 진동 없음");
        assert_eq!(
            addr.buffer, "",
            "empty-{i}: 버퍼가 경로로 재동기화되지 않음"
        );
    }

    assert_eq!(
        addr.entry_transitions, 1,
        "편집 진입 전이(false→true)는 전체 시나리오에서 클릭 1회뿐이어야 한다"
    );
    assert_eq!(
        addr.fetch_recent_calls, 1,
        "recent 캐시 조회는 편집 진입당 1회 계약"
    );
}

/// `AutoComplete`(따라서 `PathField`)의 드롭다운 origin 은 트리거 `Input` 이 반환하는
/// `Response.rect` 를 그대로 anchor 로 쓴다(`autocomplete.rs` `resp.rect.left_bottom()`).
/// 그 `rect` 가 내부 TextEdit rect(leading icon 만큼 우측으로 밀림)가 아니라 outer(테두리)
/// rect 여야, explorer/markdown 주소창처럼 항상 leading icon 을 지정하는 호출에서 드롭다운이
/// 주소 표시줄과 좌우로 어긋나지 않는다. leading icon 유무에 따라 outer rect 가 달라지면
/// (=내부 TextEdit rect 가 새는 회귀) 이 테스트가 잡는다.
#[test]
fn input_outer_rect_unaffected_by_leading_icon() {
    let theme = tasty_themes::mocha_fallback();
    let ctx = egui::Context::default();

    let dot_icon = |ui: &mut egui::Ui, rect: egui::Rect, c: egui::Color32| {
        ui.painter().circle_filled(rect.center(), 4.0, c);
    };

    let mut buf_with_icon = String::new();
    let mut buf_without_icon = String::new();
    let mut rect_with_icon = Rect::NOTHING;
    let mut rect_without_icon = Rect::NOTHING;

    let _out = ctx.run(raw(true, vec![]), |c| {
        egui::CentralPanel::default().show(c, |ui| {
            let resp =
                Input::new()
                    .icon(&dot_icon)
                    .width(200.0)
                    .show(ui, &theme, &mut buf_with_icon);
            rect_with_icon = resp.rect;

            let resp = Input::new()
                .width(200.0)
                .show(ui, &theme, &mut buf_without_icon);
            rect_without_icon = resp.rect;
        });
    });

    assert_eq!(
        rect_with_icon.width(),
        200.0,
        "outer rect 폭은 지정한 width 그대로여야 한다 \
         (버그: TextEdit 내부 rect 는 icon/padding 만큼 좁았다)"
    );
    assert_eq!(
        rect_with_icon.left(),
        rect_without_icon.left(),
        "leading icon 유무와 무관하게 outer rect 좌측 경계는 동일해야 한다 \
         (버그: icon 이 있으면 TextEdit 내부 rect 만큼 우측으로 밀렸었다)"
    );
    assert_eq!(
        rect_with_icon.height(),
        rect_without_icon.height(),
        "outer rect 높이도 icon 유무와 무관하게 동일해야 한다"
    );
}
