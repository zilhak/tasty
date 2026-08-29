//! `draw_status_bar_view` 계약 회귀 테스트 (headless egui).
//!
//! 세 가지를 고정한다:
//! ① 팔레트 칩 / 테마 토글 클릭이 각각 `OpenPalette` / `ToggleTheme` 를 보고한다.
//! ② 그 두 셀 위의 hover 가 `resize_priority_hovered` 를 세우고, 비클릭 좌측
//!    클러스터에서는 세우지 않는다(윈도우 엣지 리사이즈 우선권 판정).
//! ③ **부모 `Ui` 가 화면 원점이 아닌 임의 위치에 있어도** 셀 배치가 동일하다 —
//!    view 가 절대 화면 좌표를 쓰면 갤러리 카드(임의 y) 안에서 좌표가 어긋난다.
//!    이관 전 구현이 `rect.x_range()` / `rect.width()` 같은 절대 rect 를 직접 쓰던
//!    자리의 회귀 테스트다.

use egui::{Event, Modifiers, PointerButton, Pos2, RawInput, Rect, pos2, vec2};
use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{StatusBarAction, StatusBarData, StatusBarDrawResult, draw_status_bar_view};

const BAR_W: f32 = 600.0;
// view 의 디자인 inline 레이아웃 값(work.jsx `StatusBar`) — 셀 좌표를 테스트에서
// 독립적으로 재계산해 배치 계약을 고정한다.
const CELL_PAD_X: f32 = 10.0;
const CELL_GAP: f32 = 6.0;
const DOT_SIZE: f32 = 7.0;

const PALETTE_LABEL: &str = "Ctrl+K palette";
const THEME_ID: &str = "mocha";
/// view 의 `capitalize(theme_id)` 결과.
const THEME_LABEL: &str = "Mocha";

fn data() -> StatusBarData {
    StatusBarData {
        branch: Some("main".into()),
        surface_id: Some(3),
        shell: Some("zsh".into()),
        grid: Some((120, 32)),
        theme_id: THEME_ID.into(),
        theme_is_light: false,
        palette_label: PALETTE_LABEL.into(),
        palette_tooltip: "Open the command palette".into(),
        theme_tooltip: "Toggle theme".into(),
    }
}

fn raw(events: Vec<Event>) -> RawInput {
    RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1200.0, 800.0))),
        focused: true,
        events,
        ..Default::default()
    }
}

/// 임의 위치의 부모 `Ui` — 본체는 `egui::Area`, 갤러리는 카드 안이라 둘 다 "주어진 Ui".
fn frame(
    ctx: &egui::Context,
    theme: &Theme,
    origin: Pos2,
    d: &StatusBarData,
    events: Vec<Event>,
) -> StatusBarDrawResult {
    let mut out = StatusBarDrawResult::default();
    // FullOutput 불필요 — 이 테스트는 view 가 보고한 action/hover 만 확인한다.
    let _out = ctx.run(raw(events), |c| {
        egui::Area::new(egui::Id::new("host"))
            .fixed_pos(origin)
            .show(c, |ui| {
                out = draw_status_bar_view(ui, theme, LogicalPx(BAR_W), d);
            });
    });
    out
}

fn text_w(ctx: &egui::Context, theme: &Theme, s: &str) -> f32 {
    ctx.fonts(|f| {
        f.layout_no_wrap(
            s.to_owned(),
            egui::FontId::monospace(theme.font_size_caption.value()),
            egui::Color32::PLACEHOLDER,
        )
        .size()
        .x
    })
}

/// 우측 클러스터 두 셀의 중심 x — 우측 끝에 flush 로 붙는다(spacer 가 밀어냄).
fn right_cluster_centers(ctx: &egui::Context, theme: &Theme, origin: Pos2) -> (f32, f32) {
    let theme_w = DOT_SIZE + CELL_GAP + text_w(ctx, theme, THEME_LABEL) + CELL_PAD_X * 2.0;
    let palette_w = text_w(ctx, theme, PALETTE_LABEL) + CELL_PAD_X * 2.0;
    let right = origin.x + BAR_W;
    (right - theme_w - palette_w / 2.0, right - theme_w / 2.0)
}

fn ptr_move(p: Pos2) -> Event {
    Event::PointerMoved(p)
}

fn ptr_btn(p: Pos2, pressed: bool) -> Event {
    Event::PointerButton {
        pos: p,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::default(),
    }
}

/// hover 이동 → press → release 3 프레임. `clicked()` 는 release 프레임에 뜬다.
fn click_at(ctx: &egui::Context, theme: &Theme, origin: Pos2, p: Pos2) -> StatusBarDrawResult {
    let d = data();
    frame(ctx, theme, origin, &d, vec![ptr_move(p)]);
    frame(ctx, theme, origin, &d, vec![ptr_btn(p, true)]);
    frame(ctx, theme, origin, &d, vec![ptr_btn(p, false)])
}

fn hover_at(ctx: &egui::Context, theme: &Theme, origin: Pos2, p: Pos2) -> StatusBarDrawResult {
    let d = data();
    frame(ctx, theme, origin, &d, vec![ptr_move(p)]);
    frame(ctx, theme, origin, &d, vec![ptr_move(p)])
}

/// 배치 계산에 쓰는 폰트 metric 을 얻으려면 프레임을 한 번 돌려 font 를 초기화해야 한다.
fn warmed_ctx(theme: &Theme) -> egui::Context {
    let ctx = egui::Context::default();
    frame(&ctx, theme, Pos2::ZERO, &data(), vec![]);
    ctx
}

#[test]
fn palette_and_theme_cells_report_their_actions() {
    let theme = tasty_themes::mocha_fallback();
    let ctx = warmed_ctx(&theme);
    let origin = Pos2::ZERO;
    let (palette_x, theme_x) = right_cluster_centers(&ctx, &theme, origin);
    let y = origin.y + 12.0;

    let out = click_at(&ctx, &theme, origin, pos2(theme_x, y));
    assert_eq!(out.actions, vec![StatusBarAction::ToggleTheme]);

    let out = click_at(&ctx, &theme, origin, pos2(palette_x, y));
    assert_eq!(out.actions, vec![StatusBarAction::OpenPalette]);

    // 좌측 클러스터(브랜치 셀)는 비클릭 — 어떤 액션도 나오지 않는다.
    let out = click_at(&ctx, &theme, origin, pos2(origin.x + 15.0, y));
    assert!(out.actions.is_empty(), "좌측 클러스터는 표시 전용이다");
}

#[test]
fn hover_over_clickable_cells_sets_resize_priority() {
    let theme = tasty_themes::mocha_fallback();
    let ctx = warmed_ctx(&theme);
    let origin = Pos2::ZERO;
    let (palette_x, theme_x) = right_cluster_centers(&ctx, &theme, origin);
    let y = origin.y + 12.0;

    assert!(hover_at(&ctx, &theme, origin, pos2(theme_x, y)).resize_priority_hovered);
    assert!(hover_at(&ctx, &theme, origin, pos2(palette_x, y)).resize_priority_hovered);
    assert!(
        !hover_at(&ctx, &theme, origin, pos2(origin.x + 15.0, y)).resize_priority_hovered,
        "비클릭 좌측 클러스터는 리사이즈 우선권을 뺏지 않는다"
    );
}

/// 부모 `Ui` 가 화면 원점이 아니어도 같은 상대 좌표에서 같은 셀이 잡혀야 한다.
#[test]
fn layout_is_relative_to_the_parent_ui_not_the_screen_origin() {
    let theme = tasty_themes::mocha_fallback();
    let ctx = warmed_ctx(&theme);
    let origin = pos2(137.0, 211.0);
    let (palette_x, theme_x) = right_cluster_centers(&ctx, &theme, origin);
    let y = origin.y + 12.0;

    assert_eq!(
        click_at(&ctx, &theme, origin, pos2(theme_x, y)).actions,
        vec![StatusBarAction::ToggleTheme],
        "절대 좌표 잔재가 있으면 오프셋된 부모에서 셀이 어긋난다"
    );
    assert_eq!(
        click_at(&ctx, &theme, origin, pos2(palette_x, y)).actions,
        vec![StatusBarAction::OpenPalette]
    );
    assert!(hover_at(&ctx, &theme, origin, pos2(theme_x, y)).resize_priority_hovered);
    // 화면 원점(0,0) 근처는 이제 바 바깥이다 — 아무 셀도 잡히지 않아야 한다.
    assert!(
        !hover_at(&ctx, &theme, origin, pos2(5.0, 5.0)).resize_priority_hovered,
        "바가 옮겨갔는데 화면 원점에서 hover 가 잡히면 절대 좌표를 쓰고 있는 것이다"
    );
}
