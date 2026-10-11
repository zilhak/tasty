use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{OpStatusProps, op_status_line};

#[test]
fn counting_removes_the_track_and_fill_but_keeps_status_text() {
    let theme = Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0);
    for show_bar in [false, true] {
        let ctx = egui::Context::default();
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let rect = egui::Rect::from_min_size(ui.cursor().min, egui::vec2(520.0, 28.0));
                op_status_line(
                    ui,
                    &theme,
                    rect,
                    &OpStatusProps {
                        show_bar,
                        fraction: Some(0.5),
                        waiting: false,
                        text: "counting sentinel",
                        bytes: None,
                        queued: None,
                        show_label: "Show",
                        cancel_tip: "Cancel",
                    },
                    &|_, _, _| {},
                );
            });
        });
        let has_fill = |color| {
            output.shapes.iter().any(|shape| {
            matches!(&shape.shape, egui::Shape::Rect(rect) if rect.fill == color && rect.rect.height() == theme.explorer_progress_height().value())
        })
        };
        assert_eq!(
            has_fill(theme.explorer_progress_track().to_egui()),
            show_bar
        );
        assert_eq!(has_fill(theme.explorer_progress_fill().to_egui()), show_bar);
        assert!(output.shapes.iter().any(|shape| {
            matches!(&shape.shape, egui::Shape::Text(text) if text.galley.text() == "counting sentinel")
        }));
    }
}
