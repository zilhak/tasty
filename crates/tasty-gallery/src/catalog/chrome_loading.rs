//! Chrome · Boot loading screen — 워드마크 + 스피너 + phase 문구 중앙 스택
//! (S-17, `guidelines/brand-logo.html` 브랜드 락업 + 디자인 확정
//! `changelog/2026-07-13-startup-loading-screen.md`).
//!
//! 실 렌더 경로(`src/gfx/gpu/loading.rs::render_loading`)와 동일한 스택 구성을
//! egui 로 재현한다. 갤러리는 root 바이너리 크레이트를 의존하지 않으므로
//! (위젯/테마 크레이트만 의존) 워드마크 자산(PNG)·브랜드 색은 `src/adapters/ui/brand.rs`
//! 를 이 모듈에 로컬로 미러링한다 — 두 곳 다 같은 `assets/icons/icon_256.png` 를
//! `include_bytes!` 한다(크레이트 경계상 불가피한 자산 복제, 로직 복제 아님).

use tasty_type_appearance::color::HexColor;
use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::Spinner;

use crate::catalog::spec::{meta, note};

/// 수박 과육 (`src/adapters/ui/brand.rs::MELON_FLESH` 미러 — 근거는 그 모듈 doc).
#[allow(clippy::disallowed_methods)]
const MELON_FLESH: HexColor = HexColor::from_rgb(0xf2, 0x5d, 0x6b);

const LOGO_PNG: &[u8] = include_bytes!("../../../../assets/icons/icon_256.png");
const LOGO_URI: &str = "bytes://tasty_gallery_boot_logo_256.png";

/// 워드마크 마크 크기 — 브랜드 락업 확정값(14px 상한의 sanctioned 예외).
const WORDMARK_ICON_SIZE: LogicalPx = LogicalPx(64.0);
/// 워드마크 `tasty.` 폰트 크기 — 위와 동일 근거.
const WORDMARK_FONT_SIZE: LogicalPx = LogicalPx(38.0);
/// 스피너 boot hero 크기(디자인 확정: 기본 16 → boot 32).
const SPINNER_SIZE: LogicalPx = LogicalPx(32.0);
/// phase 문구 고정 높이 슬롯(`--tasty-size-16`).
const PHASE_SLOT_HEIGHT: LogicalPx = LogicalPx(16.0);

/// 실 부팅 로딩 화면 1장 — `canvas` 크기의 faux 창에 워드마크→스피너→phase 문구
/// 중앙 스택을 그린다. `render_loading` 과 동일하게 창 크기와 무관하게 스택
/// 자체는 고정 크기(반응형 축소 없음) — `top_pad` 로만 수직 중앙 정렬한다.
fn draw_frame(ui: &mut egui::Ui, theme: &Theme, canvas: egui::Vec2, phase_text: Option<&str>) {
    let (rect, _) = ui.allocate_exact_size(canvas, egui::Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 0.0, theme.bg_app().to_egui());

    let content_height = WORDMARK_ICON_SIZE.value()
        + theme.spacing_xl.value()
        + SPINNER_SIZE.value()
        + theme.spacing_lg.value()
        + PHASE_SLOT_HEIGHT.value();
    let top_pad = ((canvas.y - content_height) / 2.0).max(0.0);

    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::top_down(egui::Align::Center)),
    );
    child.add_space(top_pad);
    draw_wordmark(&mut child, theme);
    child.add_space(theme.spacing_xl.value());
    Spinner::new()
        .size(SPINNER_SIZE.value())
        .color(theme.accent_primary().to_egui())
        .show(&mut child, theme);
    child.add_space(theme.spacing_lg.value());
    let (slot_rect, _) = child.allocate_exact_size(
        egui::vec2(canvas.x, PHASE_SLOT_HEIGHT.value()),
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

/// 워드마크 락업 — 수박 마크 + `tasty.` mono(`.` 는 `MELON_FLESH`). 근거·구조는
/// `src/adapters/ui/brand.rs::draw_wordmark` 와 동일(호스트 앱은 그쪽을 쓴다).
fn draw_wordmark(ui: &mut egui::Ui, theme: &Theme) {
    ui.horizontal(|ui| {
        let icon_vec = egui::vec2(WORDMARK_ICON_SIZE.value(), WORDMARK_ICON_SIZE.value());
        let (icon_rect, _) = ui.allocate_exact_size(icon_vec, egui::Sense::hover());
        egui::Image::from_bytes(LOGO_URI, LOGO_PNG)
            .fit_to_exact_size(icon_vec)
            .paint_at(ui, icon_rect);
        ui.add_space(theme.spacing_sm.value());
        let mut job = egui::text::LayoutJob::default();
        let font = egui::FontId::monospace(WORDMARK_FONT_SIZE.value());
        job.append(
            "tasty",
            0.0,
            egui::TextFormat {
                font_id: font.clone(),
                extra_letter_spacing: -0.5,
                color: theme.text_primary().to_egui(),
                ..Default::default()
            },
        );
        job.append(
            ".",
            0.0,
            egui::TextFormat {
                font_id: font,
                extra_letter_spacing: -0.5,
                color: MELON_FLESH.to_egui(),
                ..Default::default()
            },
        );
        ui.label(job);
    });
}

/// 기본 — 1280×720, `GpuInit` 문구.
pub fn draw_default(ui: &mut egui::Ui, theme: &Theme) {
    draw_frame(
        ui,
        theme,
        egui::vec2(1280.0, 720.0),
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
        egui::vec2(640.0, 480.0),
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
            draw_frame(ui, theme, egui::vec2(320.0, 240.0), Some(text));
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
    draw_frame(ui, theme, egui::vec2(640.0, 480.0), None);
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
        egui::vec2(640.0, 480.0),
        Some("Restoring layout…"),
    );
    note(
        ui,
        &latte,
        "GPU clear color reads the resolved theme's bg-app — Latte follows the saved theme, not a hardcoded dark.",
    );
}
