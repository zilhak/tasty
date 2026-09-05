//! Warning callout — 디자인 Settings › Terminal › TUI 섹션 jsx:623-632 의 bordered
//! warning box. `tasty_ui_widgets::warning_callout` 위젯 1:1 데모.
//!
//! 좌측 경고 삼각 아이콘 + caption 본문을 `accent-warning` 40% 보더 / 12% 틴트
//! 배경의 라운드 박스로 감싼다. OSC 52 클립보드 읽기 토글 바로 아래에 붙어, 그 권한이
//! 무엇을 여는지 경고하는 한 블록. 아이콘은 갤러리의 `catalog::icons::ALERT_TRIANGLE`
//! 를 `IconPainter` 클로저로 주입한다(본체는 `icons::ALERT_TRIANGLE`).
//!
//! 색·보더·라운드·간격·폰트는 전부 `Theme` 토큰 경유(`from_rgb`/hex 리터럴 금지).

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{switch, warning_callout};

use crate::catalog::icons;
use crate::catalog::spec::{self, StageVariant, TokenChip};

/// 디자인 신규 문구 (본체 `allow_clipboard_read_notice` en 값과 동일).
const NOTICE: &str = "Turning this on lets programs running in the terminal read your \
     system clipboard via OSC 52. Leave it off unless you trust everything that runs here.";

/// 경고 callout 배경 tint. 대응 토큰 없음 — 값에 이름만 둔다.
const WARNING_BG_TINT_OPACITY: f32 = 0.12;

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Solo, |ui| {
        ui.scope(|ui| {
            ui.set_max_width(theme.measure_md.value());
            ui.spacing_mut().item_spacing.y = theme.spacing_sm.value();

            // faux Settings › Terminal › TUI 토글 행 — callout 이 이 바로 아래 붙는 맥락.
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("Allow clipboard read (OSC 52)")
                        .size(theme.font_size_body.value())
                        .color(theme.text_secondary().to_egui()),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let mut on = false;
                    switch(ui, theme, &mut on, None, true);
                });
            });

            warning_callout(ui, theme, NOTICE, &|ui, rect, c| {
                icons::ALERT_TRIANGLE
                    .image(rect.height(), c)
                    .paint_at(ui, rect);
            });
        });
    });

    spec::meta(
        ui,
        theme,
        &[
            ("lives in", "Settings › Terminal › TUI"),
            ("anchor", "directly under the OSC 52 toggle"),
            ("leading", "alertTriangle glyph (injected)"),
            ("body", "caption · wraps in the box width"),
            ("border", "1px · accent-warning 40%"),
            ("background", "accent-warning 12% tint"),
        ],
        &[
            TokenChip::new(
                "accent-warning",
                "border · glyph",
                theme.accent_warning().to_egui(),
            ),
            TokenChip::new(
                "warning-bg",
                "12% tint fill",
                theme
                    .accent_warning()
                    .to_egui()
                    .gamma_multiply(WARNING_BG_TINT_OPACITY),
            ),
            TokenChip::new(
                "text-secondary",
                "caption body",
                theme.text_secondary().to_egui(),
            ),
        ],
    );

    spec::do_(
        ui,
        theme,
        "Reach for a warning callout when a toggle grants a standing capability whose risk \
         isn't obvious from its label — the bordered tint binds the caution to the control \
         right above it, unlike a loose colored line of text.",
    );

    spec::note(
        ui,
        theme,
        "The icon is host-owned: this crate has no icon assets, so the callout takes an \
         IconPainter closure and the caller injects ALERT_TRIANGLE (main app) / the mock \
         glyph (gallery). The 40%/12% warning tints approximate the design's color-mix via \
         gamma_multiply, the same idiom as tinted chips and dimmed banners.",
    );
}
