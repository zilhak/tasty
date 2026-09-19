//! 갤러리의 egui 폰트 스택 — 본체와 같은 서체를 쓰기 위한 설치 한 자리.
//!
//! lib 에 있는 이유는 바이너리와 **시험이 같은 것을 설치**해야 하기 때문이다.
//! 이 크레이트의 `tests/mono_metrics.rs` 는 여기서 설치한 스택으로 글자 폭을 재서
//! 파생 상한이 참인지 본다 — 설치가 `main.rs` 안에만 있으면 그 시험이 재는 것은
//! 화면에 실제로 깔리는 폰트가 아니게 된다.

use std::sync::Arc;

/// 갤러리의 egui 폰트 스택 — 본체(`src/gfx/gpu/fonts.rs::setup_egui_fonts`)와 같은
/// 순서로 번들 D2Coding 을 `Monospace` 맨 앞에 놓고, 시스템 CJK 를 두 family 의
/// 폴백으로 붙인다.
///
/// 서체를 맞추는 것이 갤러리의 일이다 — mono 자간이 다르면 문자 폭으로 재는 specimen
/// (파일 핸들러 헤더 경로 컷 · 행 id 말줄임)이 본체와 **다른 자리에서** 잘린 그림을
/// 보이고, 그 그림으로 정합을 판정하면 틀린다. egui 기본 mono 는 11px 에서 6.71875px/자,
/// D2Coding 은 5.5px/자다.
///
/// 언어팩 폰트는 붙이지 않는다 — 갤러리는 설정을 읽지 않는다.
pub fn install(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    fonts.font_data.insert(
        "d2coding".to_owned(),
        Arc::new(egui::FontData::from_static(
            tasty_font::D2CODING_REGULAR_TTF,
        )),
    );
    fonts
        .families
        .entry(egui::FontFamily::Monospace)
        .or_default()
        .insert(0, "d2coding".to_owned());

    if let Some(cjk) = tasty_egui_theme::load_system_cjk_font() {
        fonts.font_data.insert(
            "system_cjk".to_owned(),
            Arc::new(egui::FontData::from_owned(cjk)),
        );
        for fam in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            fonts
                .families
                .entry(fam)
                .or_default()
                .push("system_cjk".to_owned());
        }
    } else {
        tracing::warn!(
            "no system CJK font found; Korean/Japanese/Chinese labels will render as \u{25a1}"
        );
    }

    ctx.set_fonts(fonts);
}
