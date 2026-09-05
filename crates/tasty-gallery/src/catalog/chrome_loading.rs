//! Chrome · Loading screen — 워드마크 + 스피너 + phase 문구 중앙 스택
//! (S-17, `guidelines/brand-logo.html` 브랜드 락업).
//!
//! **부팅과 종료가 같은 락업을 쓴다.** 실 렌더도 `render_loading` 한 벌이고 phase
//! 문구만 다르므로, 여기서도 `draw_frame` 을 공유하고 종료 specimen 은 문구만
//! 바꾼다 — 갤러리가 두 화면의 동일성을 눈으로 확인하는 자리다.
//!
//! 실 렌더 경로(`src/gfx/gpu/loading.rs::render_loading`)와 동일한 스택 구성을
//! egui 로 재현한다. 워드마크 락업(마크 PNG·브랜드 색·`draw_wordmark`)과 로딩
//! 스택 상수는 위젯 크레이트 [`tasty_ui_widgets::brand`] 의 단일 출처를 쓴다 —
//! 본체 `render_loading` 도 같은 출처를 부르므로, 이 specimen 과 실 화면이 값·렌더를
//! 공유한다(예전의 로컬 복제는 승격으로 제거).

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::Spinner;
// 워드마크 락업 상수·렌더·브랜드 색은 위젯 크레이트가 단일 출처다 — 예전엔 본체
// `src/adapters/ui/brand.rs` 값을 여기 로컬 미러링했으나 승격으로 복제를 없앴다.
use tasty_ui_widgets::brand::{self};

use crate::catalog::spec::{meta, note};

// ── 무대 canvas 치수 ─────────────────────────────────────────────────────────
//
// specimen 이 그리는 faux 부팅창의 크기다. 디자인 토큰이 아니라 **무대 크기**라
// Theme 에서 오지 않는다 — 값이 케이스마다 다른 이유는 그 케이스가 무엇을 보여야
// 하는지(기본/최소/나란히 비교)에 있다. 로딩 스택은 창 크기와 무관하게 고정
// 크기(반응형 축소 없음)라, 이 값들은 "축소가 없다"·"슬롯이 흔들리지 않는다" 를
// 눈으로 확인시키는 무대일 뿐이다.

/// 기본 부팅/종료 창 — 실 렌더가 흔히 present 하는 1280×720.
const CANVAS_DEFAULT: (f32, f32) = (1280.0, 720.0);
/// 최소창 데모 — 반응형 축소가 없음을 보이는 640×480 극단(문구 없음·Latte 변형 공용).
const CANVAS_MIN: (f32, f32) = (640.0, 480.0);
/// phase 문구 3~4종을 가로로 나란히 비교하는 좁은 무대.
const CANVAS_MULTI: (f32, f32) = (320.0, 240.0);

/// 실 부팅 로딩 화면 1장 — `canvas` 크기의 faux 창에 워드마크→스피너→phase 문구
/// 중앙 스택을 그린다. `render_loading` 과 동일하게 창 크기와 무관하게 스택
/// 자체는 고정 크기(반응형 축소 없음) — `top_pad` 로만 수직 중앙 정렬한다.
fn draw_frame(ui: &mut egui::Ui, theme: &Theme, canvas: egui::Vec2, phase_text: Option<&str>) {
    let (rect, _) = ui.allocate_exact_size(canvas, egui::Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 0.0, theme.bg_app().to_egui());

    let content_height = theme.loading_screen_wordmark_icon_size().value()
        + theme.spacing_xl.value()
        + theme.loading_screen_spinner_size().value()
        + theme.spacing_lg.value()
        + theme.loading_screen_phase_slot_height().value();
    let top_pad = ((canvas.y - content_height) / 2.0).max(0.0);

    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::top_down(egui::Align::Center)),
    );
    child.add_space(top_pad);
    brand::draw_wordmark(
        &mut child,
        theme,
        theme.loading_screen_wordmark_icon_size(),
        theme.loading_screen_wordmark_font_size(),
    );
    child.add_space(theme.spacing_xl.value());
    Spinner::new()
        .size(theme.loading_screen_spinner_size().value())
        .color(theme.accent_primary().to_egui())
        .show(&mut child, theme);
    child.add_space(theme.spacing_lg.value());
    let (slot_rect, _) = child.allocate_exact_size(
        egui::vec2(canvas.x, theme.loading_screen_phase_slot_height().value()),
        egui::Sense::hover(),
    );
    if let Some(text) = phase_text {
        child.painter().text(
            slot_rect.center(),
            egui::Align2::CENTER_CENTER,
            text,
            egui::FontId::proportional(theme.font_size_body.value()),
            theme.text_muted().to_egui(),
        );
    }
}

/// 기본 — 1280×720, `GpuInit` 문구.
pub fn draw_default(ui: &mut egui::Ui, theme: &Theme) {
    draw_frame(
        ui,
        theme,
        egui::vec2(CANVAS_DEFAULT.0, CANVAS_DEFAULT.1),
        Some("Initializing graphics…"),
    );
    meta(
        ui,
        theme,
        &[("window", "1280×720 default"), ("phase", "GpuInit")],
        &[],
    );
}

/// 최소창 — 640×480, 동일 중앙 스택(반응형 축소 없음 확인용).
pub fn draw_min(ui: &mut egui::Ui, theme: &Theme) {
    draw_frame(
        ui,
        theme,
        egui::vec2(CANVAS_MIN.0, CANVAS_MIN.1),
        Some("Loading plugins…"),
    );
    meta(
        ui,
        theme,
        &[("window", "640×480 minimum"), ("phase", "WaitingPlugins")],
        &[],
    );
    note(
        ui,
        theme,
        "Layout is size-invariant — same centered stack at both window extremes, no responsive scaling.",
    );
}

/// phase 문구 3종 — `GpuInit` / `WaitingPlugins` / `RestoringLayout` 나란히 비교.
pub fn draw_phases(ui: &mut egui::Ui, theme: &Theme) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = theme.spacing_lg.value();
        for text in [
            "Initializing graphics…",
            "Loading plugins…",
            "Restoring layout…",
        ] {
            draw_frame(
                ui,
                theme,
                egui::vec2(CANVAS_MULTI.0, CANVAS_MULTI.1),
                Some(text),
            );
        }
    });
    meta(
        ui,
        theme,
        &[
            ("GpuInit / WaitingEngine", "\"Initializing graphics…\""),
            ("WaitingPlugins", "\"Loading plugins…\""),
            ("RestoringLayout", "\"Restoring layout…\""),
        ],
        &[],
    );
}

/// 문구 없는 변형 — 슬롯은 예약되지만 비어 있다(첫 설치, RestoringLayout 스킵).
pub fn draw_no_text(ui: &mut egui::Ui, theme: &Theme) {
    draw_frame(ui, theme, egui::vec2(CANVAS_MIN.0, CANVAS_MIN.1), None);
    note(
        ui,
        theme,
        "First install can skip RestoringLayout — the phase slot stays reserved but empty, no layout shift.",
    );
}

/// Latte 변형 — 앰비언트 테마 토글과 무관하게 고정 표시(저장된 테마를 따라간다는
/// 디자인 확정 §5 를 보여주는 비교 카드).
pub fn draw_latte(ui: &mut egui::Ui, _theme: &Theme) {
    let latte = crate::host_shell::latte_theme();
    draw_frame(
        ui,
        &latte,
        egui::vec2(CANVAS_MIN.0, CANVAS_MIN.1),
        Some("Restoring layout…"),
    );
    note(
        ui,
        &latte,
        "GPU clear color reads the resolved theme's bg-app — Latte follows the saved theme, not a hardcoded dark.",
    );
}

/// 종료 phase 문구 4종 — `SavingLayout` / `ReclaimingBootWorker` / `ClosingSurfaces`
/// / `StoppingPlugins` 나란히 비교. 락업은 부팅과 완전히 동일하다(같은 렌더 경로).
pub fn draw_shutdown_phases(ui: &mut egui::Ui, theme: &Theme) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = theme.spacing_lg.value();
        for text in [
            "Saving layout…",
            "Finishing startup…",
            "Closing surfaces…",
            "Stopping plugins…",
        ] {
            draw_frame(
                ui,
                theme,
                egui::vec2(CANVAS_MULTI.0, CANVAS_MULTI.1),
                Some(text),
            );
        }
    });
    meta(
        ui,
        theme,
        &[
            ("SavingLayout", "\"Saving layout…\""),
            ("ReclaimingBootWorker", "\"Finishing startup…\""),
            ("ClosingSurfaces", "\"Closing surfaces…\""),
            ("StoppingPlugins", "\"Stopping plugins…\""),
        ],
        &[],
    );
    note(
        ui,
        theme,
        "Only the two waiting phases (ReclaimingBootWorker, StoppingPlugins) survive a frame — the other two advance within the frame they enter, so they are rarely seen.",
    );
}

/// 종료 화면 기본 — 1280×720, 실측상 거의 유일하게 보이는 문구(`StoppingPlugins`).
pub fn draw_shutdown_default(ui: &mut egui::Ui, theme: &Theme) {
    draw_frame(
        ui,
        theme,
        egui::vec2(CANVAS_DEFAULT.0, CANVAS_DEFAULT.1),
        Some("Stopping plugins…"),
    );
    meta(
        ui,
        theme,
        &[
            ("window", "1280×720 default"),
            ("phase", "StoppingPlugins"),
            ("lockup", "identical to boot"),
        ],
        &[],
    );
    note(
        ui,
        theme,
        "A shutdown with nothing to wait for never renders this frame at all — the state machine reaches Done inside its first drive.",
    );
}
