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

    /// 본문 위에 포인터를 둔 채 툴팁 대기 시간보다 길게 프레임을 돌리고, 마지막 프레임에서
    /// `text` 를 말줄임 없이 다 보이는 도형 수를 센다. 말줄임된 galley 도 `text()` 는 원문
    /// 전체를 돌려주므로 `elided` 로 거른다.
    /// 툴팁은 본문 위에 뜬다. 본문을 화면 맨 위에 두면 툴팁이 화면 안으로 밀려 포인터를 덮고
    /// hover 가 끊기므로 위쪽에 툴팁 자리를 둔다.
    fn full_text_shapes_after_hover(width: f32, text: &str) -> usize {
        const TOP: f32 = 200.0;
        let theme = Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0);
        let ctx = egui::Context::default();
        let mut count = 0;
        for i in 0..20 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width, 600.0),
                )),
                time: Some(f64::from(i) * 0.25),
                events: vec![egui::Event::PointerMoved(egui::pos2(12.0, TOP + 8.0))],
                ..Default::default()
            };
            let out = ctx.run(input, |ctx| {
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show(ctx, |ui| {
                        ui.add_space(TOP);
                        plugin_banner_body(ui, &theme, text);
                    });
            });
            count = out
                .shapes
                .iter()
                .filter(|c| {
                    matches!(&c.shape, egui::Shape::Text(t)
                        if t.galley.text() == text && !t.galley.elided)
                })
                .count();
        }
        count
    }

    #[test]
    fn hovering_a_cut_body_shows_the_full_text_in_a_tooltip() {
        assert_eq!(full_text_shapes_after_hover(160.0, LONG), 1);
    }

    #[test]
    fn hovering_an_uncut_body_shows_no_tooltip() {
        // 잘리지 않은 본문은 자기 도형 하나만 전체 글을 보인다.
        assert_eq!(full_text_shapes_after_hover(400.0, "Short plugin body."), 1);
    }

    #[test]
    fn a_short_body_is_not_cut() {
        let g = body(400.0, "Short plugin body.");
        assert_eq!(g.rows.len(), 1);
        assert!(!g.elided);
    }
}
