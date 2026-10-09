use super::*;

#[test]
fn a_match_ignores_case_and_returns_byte_bounds() {
    assert_eq!(match_range("Report.pdf", "re"), Some(0..2));
    assert_eq!(match_range("mockup-exports", "RE"), None);
    assert_eq!(match_range("mockup-exports", "po"), Some(9..11));
    assert_eq!(match_range("한글report", "rep"), Some(6..9));
    assert_eq!(match_range("notes.md", ""), None);
    assert_eq!(match_range("a", "abc"), None);
}

#[test]
fn the_bar_is_the_search_bar_height() {
    let theme = Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0);
    let ctx = egui::Context::default();
    let mut query = String::from("re");
    let mut deep = false;
    let mut height = 0.0;
    drop(ctx.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let top = ui.cursor().top();
                explorer_find_bar(
                    ui,
                    &theme,
                    ExplorerFindBar {
                        query: &mut query,
                        subfolders: Some(&mut deep),
                        status: ExplorerFindStatus::Text("2 of 5"),
                        labels: ExplorerFindLabels {
                            placeholder: "Filter this folder",
                            subfolders: "Subfolders",
                            close: "Close",
                        },
                        focus: false,
                    },
                );
                height = ui.cursor().top() - top;
            });
    }));
    assert_eq!(height, theme.explorer_search_bar_height().value());
}

#[test]
fn a_tree_row_paints_the_matching_part_of_its_label() {
    let theme = Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0);
    let match_fg = theme.explorer_match_fg().to_egui();
    let ctx = egui::Context::default();
    let painted = |query: &'static str| {
        let out = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                crate::tree_row_matching(
                    ui,
                    &theme,
                    0,
                    false,
                    false,
                    None,
                    "report.pdf",
                    query,
                    None,
                    false,
                );
            });
        });
        out.shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::Text(text) if text.galley.text() == "report.pdf" => Some(
                    text.galley
                        .job
                        .sections
                        .iter()
                        .map(|s| (s.byte_range.clone(), s.format.color == match_fg))
                        .collect::<Vec<_>>(),
                ),
                _ => None,
            })
            .next()
            .expect("label painted")
    };
    assert_eq!(painted("PO"), [(0..2, false), (2..4, true), (4..10, false)]);
    assert_eq!(painted(""), [(0..10, false)]);
}
