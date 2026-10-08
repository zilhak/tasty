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
