//! 아이콘과 줄바꿈 본문을 경고색 상자로 묶는다. 아이콘은 호출자가 전달한다.
//! 배경은 공통 tint_fill_alpha, 테두리는 공통 tint_border_alpha를 경고색에 곱한다.

use tasty_type_appearance::theme::Theme;

use crate::icon_button::IconPainter;

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
    let bg = warning.gamma_multiply(theme.tint_fill_alpha());
    let border = warning.gamma_multiply(theme.tint_border_alpha());
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
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(glyph, glyph), egui::Sense::hover());
                paint_icon(ui, rect, theme.accent_warning_glyph().to_egui());
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(text)
                            .size(theme.font_size_caption.value())
                            .color(theme.text_primary().to_egui()),
                    )
                    .wrap(),
                );
            });
        })
        .response
}

#[cfg(test)]
mod tests {
    use super::*;

    fn latte() -> Theme {
        let file = tasty_themes::ThemeFile::parse(tasty_themes::LATTE_TOML_TEXT).expect("latte");
        let (partial, is_light) = file.to_partial();
        let mut colors = tasty_themes::mocha_fallback_colors();
        colors.apply_partial(&partial);
        Theme::with_colors(colors, is_light.unwrap_or(true))
    }

    fn flatten(shape: &egui::Shape, out: &mut Vec<egui::Shape>) {
        match shape {
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| flatten(s, out)),
            other => out.push(other.clone()),
        }
    }

    /// 그린 callout 상자의 테두리 색. 채움(accent-warning × tint-fill-alpha)으로 상자를 찾는다.
    fn painted_callout_border(theme: &Theme) -> egui::Color32 {
        let ctx = egui::Context::default();
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(480.0, 320.0),
            )),
            ..Default::default()
        };
        let out = ctx.run(raw, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                warning_callout(ui, theme, "notice", &|_, _, _| {});
            });
        });
        let mut shapes = Vec::new();
        for clipped in out.shapes {
            flatten(&clipped.shape, &mut shapes);
        }
        let fill = theme
            .accent_warning()
            .to_egui()
            .gamma_multiply(theme.tint_fill_alpha());
        let borders: Vec<egui::Color32> = shapes
            .iter()
            .filter_map(|s| match s {
                egui::Shape::Rect(r) if r.fill == fill && r.stroke.width > 0.0 => {
                    Some(r.stroke.color)
                }
                _ => None,
            })
            .collect();
        assert_eq!(borders.len(), 1, "one callout box is painted");
        borders[0]
    }

    /// 경고 callout 테두리는 정본 `tint-border-alpha`를 accent-warning에 곱한다.
    #[test]
    fn painted_callout_border_reads_tint_border_alpha() {
        for theme in [tasty_themes::mocha_fallback(), latte()] {
            assert_eq!(
                painted_callout_border(&theme),
                theme
                    .accent_warning()
                    .to_egui()
                    .gamma_multiply(theme.tint_border_alpha()),
            );
        }
    }
}
