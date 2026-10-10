//! 에이전트 승인 팝업의 선택지 버튼. 본체와 갤러리가 같이 그린다.
//! 위험 선택지는 `approval-danger-*` 토큰으로 불투명 채움·1px 테두리·글자색을 쓰고,
//! hover·누름은 그 채움 위에 일반 버튼 overlay 를 얹는다.

use tasty_type_appearance::theme::Theme;

/// 선택지 하나를 egui 버튼으로 그린다. `destructive` 면 위험 선택지 색을 쓴다.
pub fn approval_choice(
    ui: &mut egui::Ui,
    theme: &Theme,
    label: &str,
    destructive: bool,
) -> egui::Response {
    let text = egui::RichText::new(label).size(theme.button_font_size().value());
    if !destructive {
        return ui.add(egui::Button::new(text));
    }
    // 전역 override_text_color 가 다른 테마의 글자색이어도 위험 선택지 글자색을 지킨다.
    let button = egui::Button::new(text.color(theme.approval_danger_fg().to_egui()));
    ui.scope(|ui| {
        let fill = theme.approval_danger_bg().to_egui();
        let stroke = egui::Stroke::new(
            theme.border_width.value(),
            theme.approval_danger_border().to_egui(),
        );
        let fg = egui::Stroke::new(
            theme.border_width.value(),
            theme.approval_danger_fg().to_egui(),
        );
        let widgets = &mut ui.visuals_mut().widgets;
        for (state, overlay) in [
            (&mut widgets.inactive, None),
            (&mut widgets.hovered, Some(theme.button_overlay_hover())),
            (&mut widgets.active, Some(theme.button_overlay_active())),
        ] {
            let f = overlay.map_or(fill, |o| fill.blend(o.to_egui_premultiplied()));
            state.bg_fill = f;
            state.weak_bg_fill = f;
            state.bg_stroke = stroke;
            state.fg_stroke = fg;
        }
        ui.add(button)
    })
    .inner
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect_fills(shapes: &[egui::epaint::ClippedShape]) -> Vec<egui::Color32> {
        shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Rect(r) => Some(r.fill),
                _ => None,
            })
            .collect()
    }

    fn paint(theme: &Theme, destructive: bool) -> Vec<egui::Color32> {
        let ctx = egui::Context::default();
        let out = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                approval_choice(ui, theme, "Allow and skip future checks", destructive);
            });
        });
        rect_fills(&out.shapes)
    }

    /// 위험 선택지만 approval-danger-bg 로 채우고, 이전의 반투명 danger 곱셈 색은 쓰지 않는다.
    #[test]
    fn only_the_destructive_choice_is_filled_with_the_danger_token() {
        let theme = Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0);
        let danger_bg = theme.approval_danger_bg().to_egui();
        assert!(paint(&theme, true).contains(&danger_bg));
        assert!(!paint(&theme, false).contains(&danger_bg));
        let retired = theme.accent_danger().to_egui().linear_multiply(0.18);
        assert!(!paint(&theme, true).contains(&retired));
    }
}
