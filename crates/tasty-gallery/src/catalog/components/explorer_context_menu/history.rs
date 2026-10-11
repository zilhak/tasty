//! History rows retain their slots and show the first configured shortcut.

use super::{Mi, render_menu};
use crate::catalog::spec::cluster;
use crate::i18n::{t, t_count, t_fmt};
use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;

// The history specimen's menu is 240 wide, including its border and padding.
const HISTORY_MENU_WIDTH: LogicalPx = LogicalPx(240.0);

fn rows(empty: bool, windows: bool, stale: bool) -> Vec<Mi> {
    let reason = t_fmt("explorer.menu.undo_stale", t("explorer.result.newer_there"));
    let undo = if empty {
        t("explorer.menu.undo_none").to_owned()
    } else if stale && windows {
        reason.clone()
    } else {
        t_fmt(
            "explorer.menu.undo",
            &t_count("explorer.menu.op_copy", 3, &["3"]),
        )
    };
    let redo = if empty {
        t("explorer.menu.redo_none").to_owned()
    } else {
        t_fmt(
            "explorer.menu.redo",
            &t_count("explorer.menu.op_move", 1, &["1"]),
        )
    };
    vec![
        Mi::Row(
            t("explorer.context_menu.paste").into(),
            "paste",
            !empty,
            None,
        ),
        Mi::Sep,
        Mi::Row(
            undo,
            "explorer_undo",
            !empty && !stale,
            (stale && !windows).then_some(reason),
        ),
        Mi::Row(redo, "explorer_redo", !empty, None),
        Mi::Sep,
        Mi::Row(
            t("explorer.context_menu.properties").into(),
            "explorer_properties",
            true,
            None,
        ),
    ]
}

pub(super) fn draw(ui: &mut egui::Ui, theme: &Theme) {
    let width = HISTORY_MENU_WIDTH.value()
        - theme.spacing_xs.value() * 2.0
        - theme.border_width.value() * 2.0;
    for (caption, empty, windows, stale) in [
        ("history — available", false, false, false),
        ("history — empty", true, false, false),
        (
            "history — stale (Linux / macOS tooltip)",
            false,
            false,
            true,
        ),
        ("history — stale (Windows label)", false, true, true),
    ] {
        cluster(ui, theme, caption, |ui| {
            render_menu(ui, theme, width, &rows(empty, windows, stale));
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_and_stale_specimens_keep_both_history_slots() {
        for (empty, windows, stale) in [
            (true, false, false),
            (false, false, true),
            (false, true, true),
        ] {
            let items = rows(empty, windows, stale);
            assert!(matches!(&items[2], Mi::Row(_, "explorer_undo", false, _)));
            assert!(matches!(&items[3], Mi::Row(_, "explorer_redo", _, _)));
            if let Mi::Row(label, _, _, tooltip) = &items[2] {
                assert_eq!(tooltip.is_some(), stale && !windows);
                if windows {
                    assert!(label.starts_with("Can't undo. "));
                }
            }
        }
    }
    #[test]
    fn history_separators_stay_between_groups_in_a_horizontal_stage() {
        let ctx = egui::Context::default();
        let theme = tasty_themes::mocha_fallback();
        let mut output = None;
        for _ in 0..2 {
            output = Some(ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        render_menu(
                            ui,
                            &theme,
                            HISTORY_MENU_WIDTH.value(),
                            &rows(true, false, false),
                        )
                    });
                });
            }));
        }
        let shapes = output.unwrap().shapes;
        let text_y = |label: &str| {
            shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.text() == label => Some(text.pos.y),
                    _ => None,
                })
                .unwrap()
        };
        let lines: Vec<_> = shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::LineSegment { points, .. } if points[0].y == points[1].y => {
                    Some(points[0].y)
                }
                _ => None,
            })
            .collect();
        assert_eq!(lines.len(), 2);
        assert!(
            text_y(t("explorer.context_menu.paste")) < lines[0]
                && lines[0] < text_y(t("explorer.menu.undo_none"))
        );
        assert!(
            text_y(t("explorer.menu.redo_none")) < lines[1]
                && lines[1] < text_y(t("explorer.context_menu.properties"))
        );
    }
}
