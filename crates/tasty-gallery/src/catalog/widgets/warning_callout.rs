//! OSC 52 클립보드 읽기 설정 아래에 표시하는 공용 경고 위젯 예제.
//! 아이콘은 호출자가 전달하고 경고색·여백·글꼴은 Theme에서 읽는다.

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{switch, warning_callout};

use crate::catalog::icons;
use crate::catalog::spec::{self, StageVariant, TokenChip};

/// 본체 allow_clipboard_read_notice의 영어 문구.
const NOTICE: &str = "Turning this on lets programs running in the terminal read your \
     system clipboard via OSC 52. Leave it off unless you trust everything that runs here.";

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Solo, |ui| {
        ui.scope(|ui| {
            ui.set_max_width(theme.measure_md.value());
            ui.spacing_mut().item_spacing.y = theme.spacing_sm.value();

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
                    .gamma_multiply(theme.tint_fill_alpha()),
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
        "The warning widget accepts an IconPainter callback. Both the main app and gallery supply the shared ALERT_TRIANGLE icon. The border and background apply gamma_multiply to the warning color at 40% and 12%.",
    );
}
