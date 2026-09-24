//! 부팅·종료 화면의 로고, 스피너, 진행 문구 예제.
//! 본체와 공통 브랜드 위젯·Theme 값을 사용하며 두 화면은 문구만 다르게 그린다.

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::Spinner;
use tasty_ui_widgets::brand::{self};

use crate::catalog::spec::{meta, note};

// 크기가 다른 예제 창에서도 로딩 요소의 크기를 유지하는지 비교한다.

/// 기본 크기 예제 창.
const CANVAS_DEFAULT: (f32, f32) = (1280.0, 720.0);
/// 최소 크기 예제 창. 문구 없음·Latte 예제에서도 사용한다.
const CANVAS_MIN: (f32, f32) = (640.0, 480.0);
/// 진행 문구들을 나란히 비교할 예제 창.
const CANVAS_MULTI: (f32, f32) = (320.0, 240.0);

/// 요소의 크기는 유지하고 주어진 창 안에서 수직 중앙에 배치한다.
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

/// 문구가 없어도 해당 영역의 높이를 유지한다.
pub fn draw_no_text(ui: &mut egui::Ui, theme: &Theme) {
    draw_frame(ui, theme, egui::vec2(CANVAS_MIN.0, CANVAS_MIN.1), None);
    note(
        ui,
        theme,
        "First install can skip RestoringLayout — the phase slot stays reserved but empty, no layout shift.",
    );
}

/// 툴바의 테마 선택과 무관하게 Latte로 그린다.
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
