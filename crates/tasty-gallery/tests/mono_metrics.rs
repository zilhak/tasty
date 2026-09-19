#![allow(clippy::let_underscore_must_use)]

//! 파생 상한이 **참인지** 실제 폰트로 잰다.
//!
//! `tasty_ui_widgets::file_handler::target_budget_chars` 의 두 갈래 중 **측정 갈래**는
//! 여기서만 자동으로 돈다. 본체의 호출부는 `egui::Ui` 를 받아야 해서 bin 유닛 시험이
//! 들 수 없고, 갤러리 specimen 은 일부러 폴백 상수를 쓴다. 그래서 이 파일이 없으면
//! "재서 65 가 나온다" 는 주장에 채널이 없다 — 상수 쪽만 시험이 지키고, 정작 화면에
//! 깔리는 값은 아무도 안 본다.
//!
//! `egui::Context` 는 창 없이도 글자를 깔 수 있다. 갤러리가 실제로 설치하는 스택
//! (`tasty_gallery::fonts::install`)을 그대로 얹고, `pixels_per_point` 는 기본 1.0 —
//! 디자인이 px 로 말하는 그 좌표계다.

use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::file_handler::target_budget_chars;
use tasty_ui_widgets::tokens::{
    FH_TARGET_ELIDE_FALLBACK, FH_TARGET_LINE_BOX, FH_TARGET_MONO_ADVANCE,
};

/// 폰트가 올라간 Context 하나. `fonts()` 는 첫 pass 전에는 비어 있어 한 번 돌려 준다.
fn ctx() -> egui::Context {
    let ctx = egui::Context::default();
    tasty_gallery::fonts::install(&ctx);
    // 한 pass 를 돌려야 `fonts()` 가 채워진다. 반환하는 `FullOutput` 은 그릴 것이
    // 없는 빈 pass 의 산출물이라 볼 것이 없다 — 필요한 것은 부수효과뿐이다.
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

/// 한 글자 늘 때의 증분이 토큰이 적어 둔 값과 같은가.
///
/// 이 시험이 [`FH_TARGET_MONO_ADVANCE`] 의 doc 주석을 값으로 바꾼다 — 주석은 "egui 가
/// 반올림해서 6 이다" 라고 말하지만, 그 말이 참인지는 소스를 읽어서는 안 보인다.
#[test]
fn a_character_costs_the_advance_the_token_records() {
    let ctx = ctx();
    let one = laid_out(&ctx, "0");
    let two = laid_out(&ctx, "00");
    assert_eq!(LogicalPx(two - one), FH_TARGET_MONO_ADVANCE);
    // 공칭 advance 와는 다르다 — 그 차가 이 lane 이 고친 것이다.
    let nominal = ctx.fonts(|f| f.glyph_width(&egui::FontId::monospace(CAPTION_PX), '0'));
    assert!(
        (nominal - 5.5556).abs() < 0.01,
        "D2Coding 11px 의 공칭 advance: {nominal}"
    );
    assert!(two - one > nominal, "깔리는 폭이 공칭보다 넓다");
}

/// 잰 값으로 예산을 구하면 폴백 상수가 나온다 — 두 갈래가 어긋나지 않는다.
#[test]
fn measuring_gives_exactly_the_derived_cap() {
    let ctx = ctx();
    let advance = laid_out(&ctx, "00") - laid_out(&ctx, "0");
    assert_eq!(
        target_budget_chars(LogicalPx(advance)),
        FH_TARGET_ELIDE_FALLBACK
    );
}

/// 예산만큼의 글자는 라인 박스에 **들어가고** 한 글자 더는 **안 들어간다**.
///
/// 앞의 둘은 산술이고 이것이 화면 주장이다 — 65 가 맞는 수라는 말은 결국 65 자가
/// 안 잘리고 66 자가 잘린다는 뜻이다.
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

/// 시안이 적은 70 은 라인 박스를 넘는다 — 고친 이유를 값으로 남긴다.
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

/// 예약된 "언제" 열이 가장 넓은 어휘를 담는가 — 열은 그리는 폰트로 재야 한다.
///
/// 시안은 이 폭을 "10 D2Coding chars @ 11px = 55" 로 도출했는데 그 산식은 위와 같은
/// 공칭 advance 를 쓴 것이라 mono 로는 59.56px 다. 슬롯이 **proportional** caption 으로
/// 그려지기 때문에 값 56 이 살아남는다 — 산식이 아니라 결과가 맞는 경우라, 그 사실을
/// 여기에 값으로 박아 둔다.
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
