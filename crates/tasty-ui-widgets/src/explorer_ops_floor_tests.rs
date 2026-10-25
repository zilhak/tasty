//! Zero and fractional text budgets exercise the real production widget and egui font layout.
use super::*;

#[test]
fn bounded_lines_use_the_actual_zero_and_subpixel_budget() {
    for width in [0.0, 0.25, 0.5, 1.0, 64.0, 203.0, 800.0] {
        let label = "Copying 日本語-한글-folder with a long name";
        let output = render(800.0, |ui, theme| {
            let left = ui.cursor().left();
            let response = bounded_line(
                ui,
                label,
                caption(theme),
                theme.text_primary().to_egui(),
                LogicalPx(width),
            );
            assert!(response.rect.is_finite() && response.rect.height() > 0.0);
            assert!(response.rect.width() <= width && response.rect.width() >= 0.0);
            assert!(response.interact_rect.left() >= left);
            assert!(response.interact_rect.right() <= left + width);
            assert!(!response.clicked());
        });
        let shapes: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.job.text == label => {
                    Some((shape.clip_rect, text))
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            shapes.len(),
            1,
            "must exercise production bounded_line at width={width}"
        );
        let (clip, text) = shapes[0];
        // egui rounds finite wrap widths before caching; allocation and clip retain the subpixel budget.
        assert_eq!(
            text.galley.job.wrap.max_width,
            width.round(),
            "layout must use egui's rounding of the actual budget"
        );
        assert!(clip.is_finite() && clip.width() <= width);
        assert_eq!(text.galley.rows.len(), 1);
        if width < 1.0 {
            assert!(text.galley.elided);
        }
    }
}

#[test]
fn zero_and_subpixel_wrapping_retains_the_same_single_row_glyph_mesh() {
    let texts = [
        "Copying folder",
        "日本語-한글",
        "i",
        ".",
        " ",
        "",
        "e\u{301}tude",
        "\u{200b}",
        "a\nsecond",
    ];
    let colors = tasty_themes::mocha_fallback_colors();
    for zoom in [0.85, 1.0, 1.2] {
        let theme = Theme::with_colors_and_zoom(colors.clone(), false, zoom);
        render(800.0, |ui, _| {
            for font in [caption(&theme), caption_mono(&theme), body(&theme)] {
                for text in texts {
                    let job = egui::text::LayoutJob::simple_singleline(
                        text.to_owned(),
                        font.clone(),
                        theme.text_primary().to_egui(),
                    );
                    let layout = |width| {
                        let mut job = job.clone();
                        job.wrap = egui::text::TextWrapping::truncate_at_width(width);
                        ui.fonts(|fonts| fonts.layout_job(job))
                    };
                    let old = layout(1.0);
                    for width in [0.0, 0.25, 0.5, 1.0] {
                        let actual = layout(width);
                        assert!(actual.rect.is_finite());
                        assert_eq!(actual.rows.len(), 1, "text={text:?}, width={width}");
                        assert_eq!(
                            actual.rows, old.rows,
                            "glyph positions and meshes differ: text={text:?}, width={width}"
                        );
                        assert_eq!(actual.mesh_bounds, old.mesh_bounds);
                        assert_eq!(actual.elided, old.elided);
                        assert_eq!(actual.rect.width().min(width), old.rect.width().min(width));
                        assert_eq!(actual.rect.height(), old.rect.height());
                        assert_eq!(actual.num_vertices, old.num_vertices);
                        assert_eq!(actual.num_indices, old.num_indices);
                    }
                }
            }
        });
    }
}
