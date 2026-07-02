//! `warning_callout` — bordered warning callout 박스 (디자인 Settings › Terminal
//! TUI 섹션 jsx:623-632).
//!
//! 좌측 경고 삼각 아이콘 + caption 본문을, 1px warning 보더 + 옅은 warning 틴트
//! 배경의 라운드 박스로 감싼다. 플레인 경고 텍스트(`accent_warning` + `.small()`)를
//! 대체해, 토글 바로 아래에 시각적으로 붙는 한 블록으로 만든다.
//!
//! 색: `border` = `accent-warning` 40% / `bg` = `accent-warning` 12% — 디자인의
//! `color-mix(in srgb, var(--tasty-accent-warning) 40%/12%, transparent)` 를
//! `gamma_multiply` 알파 감쇠로 근사한다(chip/banner 의 tinted chip·디밍 전례와
//! 동일 idiom). 보더 두께·라운드·간격·폰트는 전부 `&Theme` 토큰.
//!
//! 아이콘 시스템은 **호출측 소유** — `tasty-ui-widgets` 는 본체 icons 에 의존하지
//! 않으므로, 본체는 `icons::ALERT_TRIANGLE`, 갤러리는 `catalog::icons::ALERT_TRIANGLE`
//! 를 각각 [`IconPainter`] 클로저로 주입한다(IconButton/banner 선례).

use tasty_type_appearance::theme::Theme;

use crate::icon_button::IconPainter;

/// `color-mix(in srgb, accent-warning X%, transparent)` 근사 — 알파 감쇠.
const BORDER_MIX: f32 = 0.4;
const BG_MIX: f32 = 0.12;

/// bordered warning callout — leading 경고 삼각 아이콘 + wrapping caption 본문.
///
/// `paint_icon` 은 위젯이 계산한 정사각 `rect` 와 warning 색을 받아 글리프를 그린다
/// (본체: `|ui, rect, c| icons::ALERT_TRIANGLE.image(rect.height(), c).paint_at(ui, rect)`).
pub fn warning_callout(
    ui: &mut egui::Ui,
    theme: &Theme,
    text: &str,
    paint_icon: IconPainter<'_>,
) -> egui::Response {
    let warning = theme.accent_warning().to_egui();
    let bg = warning.gamma_multiply(BG_MIX);
    let border = warning.gamma_multiply(BORDER_MIX);
    let glyph = theme.icon_glyph_size_sm.value();
    egui::Frame::new()
        .fill(bg)
        .stroke(egui::Stroke::new(theme.border_width.value(), border))
        .corner_radius(theme.corner_radius.value())
        .inner_margin(egui::Margin::symmetric(
            theme.spacing_md.value() as i8,
            theme.spacing_sm.value() as i8,
        ))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
                // leading 경고 삼각 글리프 — warning tint.
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(glyph, glyph), egui::Sense::hover());
                paint_icon(ui, rect, warning);
                // caption 본문 — 남는 폭에서 wrap.
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(text)
                            .size(theme.font_size_caption.value())
                            .color(theme.text_secondary().to_egui()),
                    )
                    .wrap(),
                );
            });
        })
        .response
}
