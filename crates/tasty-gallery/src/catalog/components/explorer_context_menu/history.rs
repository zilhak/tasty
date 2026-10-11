//! History rows retain their slots and show the first configured shortcut.

use super::{Mi, render_menu};
use crate::catalog::spec::{StageVariant, stage, wrap_item};
use crate::i18n::{t, t_count, t_fmt};
use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;

// The history specimen's menu is 240 wide, including its border and padding.
const HISTORY_MENU_WIDTH: LogicalPx = LogicalPx(240.0);

fn rows(empty: bool, windows: bool, stale: bool) -> Vec<Mi> {
    let reason = t_fmt("explorer.menu.undo_stale", t("explorer.result.newer_there"));
    if stale {
        return vec![Mi::Row(
            if windows {
                reason.clone()
            } else {
                t_fmt(
                    "explorer.menu.undo",
                    &t_count("explorer.menu.op_move", 3, &["3"]),
                )
            },
            "explorer_undo",
            false,
            (!windows).then_some(reason),
        )];
    }
    let undo = if empty {
        t("explorer.menu.undo_none").to_owned()
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
        Mi::Row(undo, "explorer_undo", !empty, None),
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
    stage(ui, theme, StageVariant::Tight, |ui| {
        egui::Frame::new()
            .fill(egui::Color32::from(theme.bg_app()))
            .corner_radius(theme.corner_radius.value())
            .inner_margin(egui::Margin::same(
                (theme.spacing_lg + theme.spacing_xs).value() as i8,
            ))
            .show(ui, |ui| {
                ui.with_layout(
                    egui::Layout::left_to_right(egui::Align::Min)
                        .with_main_wrap(true)
                        .with_main_align(egui::Align::Center),
                    |ui| {
                        ui.spacing_mut().item_spacing = egui::Vec2::splat(theme.spacing_lg.value());
                        for (caption, empty) in
                            [("steps on both sides", false), ("empty history", true)]
                        {
                            column(ui, theme, caption, |ui| {
                                render_menu(ui, theme, width, &rows(empty, false, false));
                            });
                        }
                        column(
                            ui,
                            theme,
                            "stale — Linux / macOS (tooltip) · Windows (label)",
                            |ui| {
                                ui.spacing_mut().item_spacing.y = theme.spacing_sm.value();
                                for windows in [false, true] {
                                    render_menu(ui, theme, width, &rows(false, windows, true));
                                }
                            },
                        );
                    },
                );
            });
    });
}

fn column(ui: &mut egui::Ui, theme: &Theme, caption: &str, contents: impl FnOnce(&mut egui::Ui)) {
    wrap_item(ui, |ui| {
        ui.spacing_mut().item_spacing.y = theme.spacing_xs.value();
        ui.label(
            egui::RichText::new(caption)
                .monospace()
                .size(theme.font_size_micro.value())
                .color(egui::Color32::from(theme.text_muted())),
        );
        ui.vertical(contents);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_stage_wraps_columns_and_keeps_stale_examples_together() {
        let theme = tasty_themes::mocha_fallback();
        for width in [1000.0, 600.0] {
            let ctx = egui::Context::default();
            crate::fonts::install(&ctx);
            let mut output = None;
            for _ in 0..4 {
                output = Some(ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, 1000.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| draw(ui, &theme));
                    },
                ));
            }
            let menus: Vec<_> = output
                .unwrap()
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::Shape::Rect(rect)
                        if rect.fill == egui::Color32::from(theme.surface_raised())
                            && (rect.rect.width() - HISTORY_MENU_WIDTH.value()).abs() < 0.5 =>
                    {
                        Some(rect.rect)
                    }
                    _ => None,
                })
                .collect();
            assert_eq!(
                menus.len(),
                4,
                "four 240px menus at width {width}: {menus:?}"
            );
            let [available, empty, gtk, windows] = menus[..] else {
                unreachable!()
            };
            assert!((available.top() - empty.top()).abs() < 0.5);
            assert!(available.right() < empty.left());
            assert!((gtk.left() - windows.left()).abs() < 0.5);
            assert!((windows.top() - gtk.bottom() - theme.spacing_sm.value()).abs() < 0.5);
            assert!(gtk.height() < available.height() / 2.0);
            assert!((gtk.height() - windows.height()).abs() < 0.5);
            if width == 1000.0 {
                assert!((available.top() - gtk.top()).abs() < 0.5);
                assert!(empty.right() < gtk.left());
            } else {
                assert!(gtk.top() > available.bottom());
            }
            assert!(menus.iter().all(|menu| menu.right() <= width));
        }
    }

    #[test]
    fn empty_slots_and_compact_stale_specimens_match_the_design() {
        for empty in [false, true] {
            let items = rows(empty, false, false);
            assert_eq!(items.len(), 6);
            assert!(
                matches!(&items[2], Mi::Row(_, "explorer_undo", enabled, _) if *enabled == !empty)
            );
            assert!(
                matches!(&items[3], Mi::Row(_, "explorer_redo", enabled, _) if *enabled == !empty)
            );
        }
        for windows in [false, true] {
            let items = rows(false, windows, true);
            assert_eq!(items.len(), 1);
            assert!(matches!(&items[0], Mi::Row(_, "explorer_undo", false, _)));
            if let Mi::Row(label, _, _, tooltip) = &items[0] {
                assert_eq!(tooltip.is_some(), !windows);
                if windows {
                    assert!(label.starts_with("Can't undo. "));
                } else {
                    assert_eq!(label, "Undo moving 3 items");
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
