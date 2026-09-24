// let _는 글꼴 설치용 egui pass의 FullOutput을 버린다. 이 검사는 렌더링 결과 대신 측정값을 사용한다.
#![allow(clippy::let_underscore_must_use)]

//! 갤러리의 실제 폰트 구성과 배율 1에서 파일 핸들러의 글자 수·시간 열 폭을 검증한다.
//! 공칭 글리프 폭과 egui가 배치한 문자열 폭을 구분하며 다른 폰트·배율까지 보장하지 않는다.

use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::file_handler::target_budget_chars;
use tasty_ui_widgets::tokens::{
    FH_TARGET_ELIDE_FALLBACK, FH_TARGET_LINE_BOX, FH_TARGET_MONO_ADVANCE,
};

/// 폰트가 올라간 Context 하나. `fonts()` 는 첫 pass 전에는 비어 있어 한 번 돌려 준다.
fn ctx() -> egui::Context {
    let ctx = egui::Context::default();
    tasty_gallery::fonts::install(&ctx);
    let _ = ctx.run(egui::RawInput::default(), |_| {});
    ctx
}

/// caption(11px) mono 로 `s` 를 깔았을 때의 가로 폭.
fn laid_out(ctx: &egui::Context, s: &str) -> f32 {
    let font = egui::FontId::monospace(CAPTION_PX);
    ctx.fonts(|f| {
        f.layout_no_wrap(s.to_owned(), font, egui::Color32::WHITE)
            .rect
            .width()
    })
}

/// 헤더 경로 줄의 폰트 크기 — `component.font-size-caption`.
const CAPTION_PX: f32 = 11.0;

#[test]
fn the_pixels_per_point_this_file_reasons_in_is_one() {
    // 아래 모든 px 주장이 논리 px 라는 전제. 여기가 1 이 아니면 그 전제가 깨진다.
    assert_eq!(ctx().pixels_per_point(), 1.0);
}

/// egui가 배치한 문자열의 한 글자 증가분과 토큰 값을 비교한다.
#[test]
fn a_character_costs_the_advance_the_token_records() {
    let ctx = ctx();
    let one = laid_out(&ctx, "0");
    let two = laid_out(&ctx, "00");
    assert_eq!(LogicalPx(two - one), FH_TARGET_MONO_ADVANCE);
    let nominal = ctx.fonts(|f| f.glyph_width(&egui::FontId::monospace(CAPTION_PX), '0'));
    assert!(
        (nominal - 5.5556).abs() < 0.01,
        "D2Coding 11px 의 공칭 advance: {nominal}"
    );
    assert!(
        two - one > nominal,
        "배치된 문자열의 증가분이 공칭 글리프 폭보다 커야 한다"
    );
}

/// 현재 폰트·배율에서 계산한 글자 수가 fallback과 같은지 확인한다.
#[test]
fn measuring_gives_exactly_the_derived_cap() {
    let ctx = ctx();
    let advance = laid_out(&ctx, "00") - laid_out(&ctx, "0");
    assert_eq!(
        target_budget_chars(LogicalPx(advance)),
        FH_TARGET_ELIDE_FALLBACK
    );
}

/// 허용 글자 수는 들어가고 한 글자를 더하면 넘치는지 실제 폭으로 확인한다.
#[test]
fn the_budget_is_the_last_count_that_still_fits_the_line_box() {
    let ctx = ctx();
    let box_w = FH_TARGET_LINE_BOX.value();
    let fits: String = "m".repeat(FH_TARGET_ELIDE_FALLBACK);
    let over: String = "m".repeat(FH_TARGET_ELIDE_FALLBACK + 1);
    assert!(
        laid_out(&ctx, &fits) <= box_w,
        "{FH_TARGET_ELIDE_FALLBACK} 자 = {}px, 라인 박스 {box_w}px",
        laid_out(&ctx, &fits)
    );
    assert!(
        laid_out(&ctx, &over) > box_w,
        "{} 자 = {}px, 라인 박스 {box_w}px",
        FH_TARGET_ELIDE_FALLBACK + 1,
        laid_out(&ctx, &over)
    );
}

/// 70글자는 현재 폰트·배율에서 줄의 폭을 넘는다.
#[test]
fn the_seventy_the_design_wrote_overflows_the_line_box() {
    let ctx = ctx();
    let w = laid_out(&ctx, &"m".repeat(70));
    assert!(
        w > FH_TARGET_LINE_BOX.value(),
        "70 자 = {w}px, 라인 박스 {}px",
        FH_TARGET_LINE_BOX.value()
    );
}

/// 실제로 사용하는 proportional caption 글꼴로 시간 열의 표본 문구가 들어가는지 확인한다.
#[test]
fn the_reserved_when_column_holds_the_widest_word() {
    let ctx = ctx();
    let theme = tasty_type_appearance::theme::Theme::with_colors(
        tasty_themes::mocha_fallback_colors(),
        false,
    );
    let reserved = theme.fh_when_width().value();
    assert_eq!(reserved, 56.0);
    let font = egui::FontId::proportional(CAPTION_PX);
    for word in [
        "just now",
        "59m ago",
        "23h ago",
        "yesterday",
        "6d ago",
        "2026-09-13",
    ] {
        let w = ctx.fonts(|f| {
            f.layout_no_wrap(word.to_owned(), font.clone(), egui::Color32::WHITE)
                .rect
                .width()
        });
        assert!(w <= reserved, "{word} = {w}px > 예약 {reserved}px");
    }
}
