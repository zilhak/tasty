//! 창 포커스와 위젯의 편집 상태를 구분하는 검사.
//! 빈 입력·focused=false인 재그리기에서도 편집 진입이 반복되지 않아야 한다.
//! 현재 Markdown의 HTML 주소창을 재현하는 검사는 아니다.

use egui::{Event, Modifiers, PointerButton, Pos2, RawInput, Rect, pos2, vec2};
use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{Input, PathField, PathFieldOutcome};

const FILE_PATH: &str = "E:/docs/readme.md";

/// 경로 필드의 상태와 편집 진입 횟수.
struct AddrState {
    buffer: String,
    editing: bool,
    active: Option<usize>,
    recent: Vec<String>,
    /// 편집 진입에 따른 후보 조회 횟수.
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

/// 고정 높이의 테스트용 표시줄에 PathField를 그린다.
/// 실제 제품 표시줄의 크기나 픽셀 정합은 검사하지 않는다.
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

/// 화면 영역·창 포커스·이벤트를 포함한 테스트 입력.
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

// 위 표시줄 안의 클릭 위치. 실제 편집 진입 여부로 이 위치를 확인한다.
const FX: f32 = 200.0;
const FY: f32 = 20.0;

/// 편집 진입 후 focused=false로 한 번 더 그리는 조건을 만든다.
/// 재그리기가 편집 진입을 반복시키지 않는지 위젯만 검사한다.
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

    // 창 포커스가 없는 빈 재그리기에서도 위젯의 편집 상태는 유지돼야 한다.
    let _out = ctx.run(raw(false, vec![]), |c| draw(c, &theme, &mut addr));
    assert!(
        addr.editing,
        "focused=false 재-run 프레임에서 editing 이 떨어지면 진동 루프가 재점화된다"
    );
}

/// 클릭 뒤 12프레임 동안 편집 진입·후보 조회가 한 번만 발생하고 버퍼가 유지되는지 확인한다.
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

/// 입력 응답은 내부 TextEdit이 아닌 필드 전체 영역이어야 한다.
/// 아이콘 유무에 따라 팝오버 앵커가 밀리지 않는지 비교한다.
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
        "필드 전체 폭은 지정한 width와 같아야 한다"
    );
    assert_eq!(
        rect_with_icon.left(),
        rect_without_icon.left(),
        "아이콘 유무와 무관하게 필드의 왼쪽 경계는 같아야 한다"
    );
    assert_eq!(
        rect_with_icon.height(),
        rect_without_icon.height(),
        "outer rect 높이도 icon 유무와 무관하게 동일해야 한다"
    );
}
