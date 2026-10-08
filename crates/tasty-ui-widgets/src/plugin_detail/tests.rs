use super::*;

fn theme() -> Theme {
    Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0)
}

/// 폭 `width` 인 Ui 에 `draw` 를 그리고 차지한 사각형을 돌려준다.
fn drawn_rect(width: f32, draw: impl Fn(&mut egui::Ui, &Theme)) -> egui::Rect {
    let theme = theme();
    let ctx = egui::Context::default();
    let mut out = None;
    for _ in 0..2 {
        drop(ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let inner = ui.allocate_ui(egui::vec2(width, f32::INFINITY), |ui| {
                    ui.set_max_width(width);
                    ui.spacing_mut().item_spacing.y = 0.0;
                    draw(ui, &theme);
                });
                out = Some(inner.response.rect);
            });
        }));
    }
    out.expect("drawn")
}

#[test]
fn the_action_bar_is_as_tall_as_its_height_function() {
    let theme = theme();
    let rect = drawn_rect(540.0, |ui, theme| {
        plugin_detail_bar(
            ui,
            theme,
            &PluginDetailBarView {
                enabled: true,
                enabled_label: "Enabled",
                disabled_label: "Disabled",
                configure: "Configure",
                uninstall: "Uninstall",
            },
        );
    });
    assert_eq!(rect.height(), plugin_detail_bar_height(&theme));
}

#[test]
fn a_command_row_is_one_settings_row_tall_including_its_bottom_line() {
    let theme = theme();
    for keys in [Some("Ctrl+Shift+V"), None] {
        let rect = drawn_rect(380.0, |ui, theme| {
            plugin_command_row(ui, theme, "Open clipboard viewer", keys);
        });
        assert_eq!(
            rect.height(),
            theme.settings_row_min_height().value(),
            "keys {keys:?}"
        );
    }
}

#[test]
fn a_long_command_title_stays_inside_the_column() {
    let rect = drawn_rect(200.0, |ui, theme| {
        plugin_command_row(
            ui,
            theme,
            "a very long command title that cannot fit in the detail column at all",
            Some("Ctrl+Alt+G"),
        );
    });
    assert!(rect.width() <= 200.0 + 0.5, "width {}", rect.width());
}

/// 그린 글자와 그 사각형. 버튼 라벨 위치로 바 안 배치를 잰다.
fn text_rects(shapes: &[egui::epaint::ClippedShape]) -> Vec<(String, egui::Rect)> {
    fn walk(shape: &egui::Shape, out: &mut Vec<(String, egui::Rect)>) {
        match shape {
            egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
            egui::Shape::Text(t) => out.push((
                t.galley.text().to_string(),
                t.galley.rect.translate(t.pos.to_vec2()),
            )),
            _ => {}
        }
    }
    let mut out = Vec::new();
    for c in shapes {
        walk(&c.shape, &mut out);
    }
    out
}

#[test]
fn the_action_buttons_end_at_the_bar_inset_and_follow_screen_order() {
    let theme = theme();
    let ctx = egui::Context::default();
    let width = 540.0;
    let mut bar_rect = egui::Rect::NOTHING;
    let mut output = None;
    for _ in 0..2 {
        output = Some(ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show(ctx, |ui| {
                    let inner = ui.allocate_ui(egui::vec2(width, f32::INFINITY), |ui| {
                        ui.set_max_width(width);
                        plugin_detail_bar(
                            ui,
                            &theme,
                            &PluginDetailBarView {
                                enabled: true,
                                enabled_label: "Enabled",
                                disabled_label: "Disabled",
                                configure: "Configure",
                                uninstall: "Uninstall",
                            },
                        );
                    });
                    bar_rect = inner.response.rect;
                });
        }));
    }
    let texts = text_rects(&output.expect("drawn").shapes);
    let find = |label: &str| {
        texts
            .iter()
            .find(|(t, _)| t == label)
            .map(|(_, r)| *r)
            .unwrap_or_else(|| panic!("{label} not drawn"))
    };
    let enabled = find("Enabled");
    let configure = find("Configure");
    let uninstall = find("Uninstall");
    assert!(enabled.right() < configure.left());
    assert!(configure.right() < uninstall.left());
    let pad_x = ControlSize::Md.pad_x(&theme);
    // 응답 rect 는 내용 폭으로 줄어들 수 있어 바에 준 폭을 기준으로 잰다.
    let expected_right = bar_rect.left() + width - PLUGIN_ADD_INSET.value();
    assert!(
        (uninstall.right() + pad_x - expected_right).abs() <= 1.0,
        "uninstall button ends at {} but the bar inset is at {expected_right}",
        uninstall.right() + pad_x
    );
}

#[test]
fn the_description_wraps_within_measure_lg_in_a_wide_column() {
    let theme = theme();
    let long =
        "A plugin description that is much longer than one line of the detail column. ".repeat(4);
    let rect = drawn_rect(900.0, |ui, theme| {
        plugin_detail_description(ui, theme, &long);
    });
    assert!(
        rect.width() <= theme.measure_lg.value() + 0.5,
        "description is {} wide",
        rect.width()
    );
    assert!(
        rect.height() > theme.font_size_body.value() * 2.0,
        "it wraps"
    );
}
