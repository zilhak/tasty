use super::*;

/// Inspect the text shapes emitted by the real Label/Ui path, including egui's wrap resolution.
#[test]
fn emitted_switch_captions_fit_the_measure_and_keep_mixed_ink() {
    let theme = tasty_themes::mocha_fallback();
    let caption = "Flags a process whose memory keeps rising across consecutive samples while above the minimum size. This explanation must wrap within the caption measure even when the settings content column is wider.";
    for width in [
        theme.settings_content_max_width().value(),
        theme.measure_md.value() / 2.0,
    ] {
        let ctx = egui::Context::default();
        let ids = ["rss_surge", "slow_loop", "call_burst"];
        let sections = ids.map(|detector| AnomalySection {
            title: "Section",
            caption,
            detector,
            rows: [
                SettingsRow::new("Notify"),
                SettingsRow::new("First"),
                SettingsRow::new("Second"),
            ],
        });
        let mut output = egui::FullOutput::default();
        let mut controls = 0;
        for _ in 0..3 {
            controls = 0;
            output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1000.0, 2000.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::Area::new(egui::Id::new("settings"))
                        .fixed_pos(egui::Pos2::ZERO)
                        .fade_in(false)
                        .show(ctx, |ui| {
                            ui.set_width(width);
                            ui.set_max_width(width);
                            settings_anomaly(
                                ui,
                                &theme,
                                "Notice",
                                "Detector",
                                &sections,
                                |ui, _, _| {
                                    controls += 1;
                                    ui.label("Control");
                                },
                            );
                        });
                },
            );
        }
        assert_eq!(controls, 9, "caption wrapping must retain every control");
        let captions: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text().starts_with(caption) => Some(text),
                _ => None,
            })
            .collect();
        assert_eq!(
            captions.len(),
            3,
            "inspect emitted captions, not LayoutJob inputs"
        );
        for (text, detector) in captions.into_iter().zip(ids) {
            let painted = text.galley.rect.translate(text.pos.to_vec2());
            assert!(
                painted.width() <= theme.measure_md.value().min(width) + 0.5,
                "rendered caption exceeds measure/available width: {painted:?}, column {width}"
            );
            assert!(
                painted.right() <= text.pos.x + width + 0.5,
                "caption overflows column"
            );
            assert!(text.galley.rows.len() > 1, "long caption wraps");
            assert_eq!(text.galley.text().matches(detector).count(), 1);
            let sections = &text.galley.job.sections;
            assert_eq!(sections.len(), 3);
            assert_eq!(sections[0].format.color, theme.text_muted().to_egui());
            assert_eq!(sections[1].format.color, theme.text_secondary().to_egui());
            assert_eq!(
                sections[1].format.font_id.family,
                egui::FontFamily::Monospace
            );
            assert_eq!(
                &text.galley.job.text[sections[1].byte_range.clone()],
                detector
            );
            assert_eq!(sections[2].format.color, theme.text_muted().to_egui());
        }
    }
}
