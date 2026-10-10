//! plugin이 제공한 배너 본문. 세 줄까지 감싸고 넘치면 말줄임하며, 잘렸을 때만 hover 툴팁에 전체 문구를 보인다.
//! Tasty 고정 문구의 배너는 이 위젯을 쓰지 않고 줄 수를 제한하지 않는다.
//! 규칙: docs/design/systems/banner.md#본문-줄-수.

use tasty_type_appearance::theme::Theme;

use crate::tooltip::{Tooltip, tooltip_hover_delay_elapsed};

/// plugin 본문이 보이는 최대 줄 수. 디자인이 정한 규칙 상수이며 토큰이 아니다.
pub const PLUGIN_BANNER_BODY_MAX_ROWS: usize = 3;

fn body_galley(
    ui: &egui::Ui,
    theme: &Theme,
    text: &str,
    wrap_width: f32,
) -> std::sync::Arc<egui::Galley> {
    let size = theme.banner_body_font_size().value();
    let mut job = egui::text::LayoutJob::single_section(
        text.to_owned(),
        egui::TextFormat {
            font_id: egui::FontId::proportional(size),
            color: theme.text_muted().to_egui(),
            line_height: Some(size * theme.line_height_ui),
            ..Default::default()
        },
    );
    job.wrap.max_width = wrap_width;
    job.wrap.max_rows = PLUGIN_BANNER_BODY_MAX_ROWS;
    ui.fonts(|f| f.layout_job(job))
}

/// 남은 폭에 본문을 그린다. 세 줄을 넘으면 셋째 줄 끝을 말줄임하고 hover 때 전체 문구를 툴팁으로 보인다.
pub fn plugin_banner_body(ui: &mut egui::Ui, theme: &Theme, text: &str) -> egui::Response {
    let galley = body_galley(ui, theme, text, ui.available_width());
    let clamped = galley.elided;
    let (rect, resp) = ui.allocate_exact_size(galley.size(), egui::Sense::hover());
    ui.painter()
        .galley(rect.min, galley, theme.text_muted().to_egui());
    if clamped && tooltip_hover_delay_elapsed(ui.ctx(), theme, resp.id, resp.hovered()) {
        Tooltip::new(text).id_source(resp.id).show(ui, theme, rect);
    }
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    const LONG: &str = "This plugin body keeps going for a while so that a narrow banner has to \
                        wrap it onto many rows, well past the three rows a plugin body may use \
                        before the rest is cut and moved into the tooltip.";

    fn body(width: f32, text: &str) -> std::sync::Arc<egui::Galley> {
        let theme = Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0);
        let ctx = egui::Context::default();
        let mut out = None;
        drop(ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                plugin_banner_body(ui, &theme, text);
                out = Some(body_galley(ui, &theme, text, width));
            });
        }));
        out.expect("galley")
    }

    #[test]
    fn a_long_body_stops_at_three_rows_with_an_ellipsis() {
        let g = body(160.0, LONG);
        assert_eq!(g.rows.len(), PLUGIN_BANNER_BODY_MAX_ROWS);
        assert!(g.elided);
    }

    #[test]
    fn a_short_body_is_not_cut() {
        let g = body(400.0, "Short plugin body.");
        assert_eq!(g.rows.len(), 1);
        assert!(!g.elided);
    }
}
