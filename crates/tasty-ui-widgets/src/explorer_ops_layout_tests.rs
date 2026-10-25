//! Render the production widgets with the actual narrow cell clip, not a resized preferred width.
use super::*;

fn theme() -> Theme {
    Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0)
}

fn render(width: f32, draw: impl FnMut(&mut egui::Ui, &Theme)) -> egui::FullOutput {
    let ctx = egui::Context::default();
    let theme = theme();
    let mut draw = draw;
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1200.0, 600.0),
            )),
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show(ctx, |ui| {
                    let rect = egui::Rect::from_min_size(
                        egui::pos2(377.0, 100.0),
                        egui::vec2(width, 400.0),
                    );
                    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
                    child.set_clip_rect(rect);
                    draw(&mut child, &theme);
                });
        },
    )
}

#[test]
fn queue_preferred_width_fits_the_actual_clip_and_preserves_right_margin() {
    for width in [203.0, 240.0, 600.0] {
        render(width, |ui, theme| {
            let anchor = egui::pos2(ui.max_rect().right() - theme.spacing_sm.value(), 480.0);
            let (rect, _) = op_queue_popover(
                ui,
                theme,
                ui.id().with("queue"),
                anchor,
                &[
                    QueueRowProps {
                        title: "Copy 1 item to beta-日本語-フォルダ",
                        sub: "108.0 KB",
                        running: true,
                        remove_tip: "Cancel",
                    },
                    QueueRowProps {
                        title: "Move 1 item to Trash",
                        sub: "Queued",
                        running: false,
                        remove_tip: "Remove",
                    },
                ],
                Some(&QueueHeader {
                    before: "On ",
                    host: HostMark {
                        name: "127.0.0.1:34217",
                        paint_glyph: &|_, _, _| {},
                    },
                    after: " · one at a time",
                }),
                &|_, _, _| {},
                &|_, _, _| {},
            );
            assert!(
                rect.left() >= ui.clip_rect().left(),
                "queue must not be chopped by the owning cell: {rect:?}"
            );
            assert!(
                (rect.right() - anchor.x).abs() < 0.01,
                "keep the existing right margin"
            );
            assert!(
                rect.width() <= op_queue_width(theme).value() + 0.01,
                "retain preferred width on wide cells"
            );
        });
    }
}

#[test]
fn narrow_status_texts_and_host_have_disjoint_visible_bounds() {
    let output = render(203.0, |ui, theme| {
        let rect = egui::Rect::from_min_size(
            egui::pos2(377.0, 400.0),
            egui::vec2(203.0, theme.item_height_interactive.value()),
        );
        op_status_line(
            ui,
            theme,
            rect,
            &OpStatusProps {
                show_bar: true,
                fraction: Some(0.0),
                waiting: false,
                text: "Copying folder",
                bytes: Some("0 / 108.0 KB"),
                queued: Some("+1 queued"),
                show_label: "Show",
                cancel_tip: "Cancel",
                host: Some(HostMark {
                    name: "127.0.0.1:34217",
                    paint_glyph: &|_, _, _| {},
                }),
            },
            &|_, _, _| {},
        );
    });
    let text: Vec<_> = output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(t) => {
                let rect =
                    egui::Rect::from_min_size(t.pos, t.galley.size()).intersect(shape.clip_rect);
                (!rect.is_negative()).then_some((t.galley.job.text.clone(), rect))
            }
            _ => None,
        })
        .collect();
    assert!(
        text.iter().any(|(s, _)| s == "+1 queued"),
        "queue control stays readable"
    );
    assert!(
        text.iter()
            .any(|(s, rect)| s == "0 / 108.0 KB" && rect.width() > 0.0),
        "bytes must retain a visible, hoverable budget"
    );
    for (i, (label, a)) in text.iter().enumerate() {
        for (other, b) in &text[i + 1..] {
            assert!(
                !a.intersects(*b),
                "{label:?} overlaps {other:?}: {a:?} vs {b:?}"
            );
        }
    }
}

#[test]
fn wide_status_preserves_full_host_bytes_queue_and_operation() {
    let output = render(800.0, |ui, theme| {
        let rect = egui::Rect::from_min_size(
            egui::pos2(377.0, 400.0),
            egui::vec2(800.0, theme.item_height_interactive.value()),
        );
        op_status_line(
            ui,
            theme,
            rect,
            &OpStatusProps {
                show_bar: false,
                fraction: None,
                waiting: false,
                text: "Copying folder",
                bytes: Some("0 / 108.0 KB"),
                queued: Some("+1 queued"),
                show_label: "Show",
                cancel_tip: "Cancel",
                host: Some(HostMark {
                    name: "127.0.0.1:34217",
                    paint_glyph: &|_, _, _| {},
                }),
            },
            &|_, _, _| {},
        );
    });
    for expected in [
        "Copying folder",
        "0 / 108.0 KB",
        "+1 queued",
        "127.0.0.1:34217",
    ] {
        assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(t) if t.galley.job.text == expected && !t.galley.elided && shape.clip_rect.contains_rect(egui::Rect::from_min_size(t.pos, t.galley.size())))), "wide control must retain {expected}");
    }
}

#[test]
fn a_host_budget_smaller_than_the_ellipsis_cannot_paint_into_its_neighbor() {
    for width in [0.0, 1.0, 8.0, 14.0, 24.0] {
        let output = render(203.0, |ui, theme| {
            bounded_host(
                ui,
                theme,
                &HostMark {
                    name: "long-remote-host.example",
                    paint_glyph: &|ui, rect, color| {
                        ui.painter().rect_filled(rect, 0.0, color);
                    },
                },
                LogicalPx(width),
            );
        });
        let color = theme().workspace_mirror_fg().to_egui();
        // The CentralPanel frame has a full-cell clip; inspect this widget’s own paint.
        let host_shapes: Vec<_> = output
            .shapes
            .iter()
            .filter(|s| match &s.shape {
                egui::Shape::Text(_) => true,
                egui::Shape::Rect(rect) => rect.fill == color,
                _ => false,
            })
            .collect();
        if width > 0.0 {
            assert!(
                !host_shapes.is_empty(),
                "must exercise the actual host painter"
            );
        }
        assert!(
            host_shapes
                .iter()
                .all(|s| s.clip_rect.right() <= 377.0 + width),
            "even the glyph/ellipsis must honor a tiny width budget: width={width}, {host_shapes:?}"
        );
    }
}

#[test]
fn a_narrow_status_exposes_the_full_host_when_its_glyph_is_hovered() {
    let ctx = egui::Context::default();
    ctx.style_mut(|style| style.interaction.tooltip_delay = 0.0);
    let theme = theme();
    let name = "unabridged-remote-host.example:34567";
    let rect = egui::Rect::from_min_size(
        egui::pos2(100.0, 100.0),
        egui::vec2(203.0, theme.item_height_interactive.value()),
    );
    let pointer = egui::pos2(
        rect.left() + status_pad(&theme) + theme.workspace_mirror_icon_size().value() * 0.5,
        rect.center().y,
    );
    let mut output = None;
    for time in [0.0, 1.0, 2.0] {
        output = Some(ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(600.0, 400.0),
                )),
                time: Some(time),
                events: vec![egui::Event::PointerMoved(pointer)],
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show(ctx, |ui| {
                        op_status_line(
                            ui,
                            &theme,
                            rect,
                            &OpStatusProps {
                                show_bar: true,
                                fraction: Some(0.0),
                                waiting: false,
                                text: "Copying 1 of 1:",
                                bytes: Some("0 B / 108.0 KB"),
                                queued: Some("+1 queued"),
                                show_label: "Show",
                                cancel_tip: "Cancel",
                                host: Some(HostMark {
                                    name,
                                    paint_glyph: &|_, _, _| {},
                                }),
                            },
                            &|_, _, _| {},
                        );
                    });
            },
        ));
    }
    assert!(output.unwrap().shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.job.text == name && !t.galley.elided)), "the hovered status glyph must expose the entire shortened name");
}
