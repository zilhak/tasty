use super::*;

fn theme() -> Theme {
    Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0)
}

fn labels() -> ExplorerCommandLabels<'static> {
    ExplorerCommandLabels {
        new_folder: "New folder",
        new_file: "New file",
        more: "More",
        cannot_write: "Can't write to this folder",
    }
}

fn find(active: bool) -> [ExplorerToggle<'static>; 1] {
    [ExplorerToggle {
        command: ExplorerCommand::Find,
        icon: tasty_icons::SEARCH,
        label: "Find",
        active,
    }]
}

/// 묶음을 그려 차지한 폭을 돌려준다.
fn drawn_width(view: &ExplorerCommandsView<'_>) -> f32 {
    let theme = theme();
    let ctx = egui::Context::default();
    let mut width = 0.0;
    drop(ctx.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ctx, |ui| {
                let before = ui.cursor().left();
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    explorer_commands(ui, &theme, view);
                    width = ui.cursor().left() - before;
                });
            });
    }));
    width
}

#[test]
fn the_reserved_width_is_the_drawn_width() {
    let theme = theme();
    let toggles = find(false);
    for create in [Some(true), Some(false), None] {
        let view = ExplorerCommandsView {
            create,
            toggles: &toggles,
            compact: false,
            labels: labels(),
        };
        let reserved = explorer_commands_width(&theme, &view);
        let drawn = drawn_width(&view);
        assert!(
            (reserved - drawn).abs() < 0.5,
            "create {create:?}: reserved {reserved}, drawn {drawn}"
        );
    }
}

#[test]
fn the_groups_fold_into_one_more_button_in_a_narrow_cell() {
    let theme = theme();
    let compact_below = theme.explorer_toolbar_compact_below().value();
    assert!(explorer_commands_compact(&theme, compact_below - 1.0));
    assert!(!explorer_commands_compact(&theme, compact_below));
    let toggles = find(true);
    let view = ExplorerCommandsView {
        create: Some(true),
        toggles: &toggles,
        compact: true,
        labels: labels(),
    };
    assert_eq!(
        explorer_commands_width(&theme, &view),
        ControlSize::Sm.height(&theme)
    );
}

#[test]
fn a_remote_explorer_shows_only_the_view_group() {
    let theme = theme();
    let toggles = find(false);
    let view = ExplorerCommandsView {
        create: None,
        toggles: &toggles,
        compact: false,
        labels: labels(),
    };
    assert_eq!(
        explorer_commands_width(&theme, &view),
        ControlSize::Sm.height(&theme)
    );
}
