//! Real ScrollArea state across layout changes, including the end-pass callback.

use super::*;
use tasty_type_geometry::length::LogicalPx;

struct Harness {
    ctx: egui::Context,
    frame: u32,
    table: bool,
    ids: [Option<egui::Id>; 2],
}

impl Harness {
    fn new(table: bool) -> Self {
        Self {
            ctx: egui::Context::default(),
            frame: 0,
            table,
            ids: [None; 2],
        }
    }

    fn frame(&mut self, rects: &[(usize, f32, f32)], held: bool, wheel: bool) -> [f32; 2] {
        let pos = egui::pos2(200.0, 540.0);
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1200.0, 1800.0),
            )),
            time: Some(f64::from(self.frame) / 60.0),
            ..Default::default()
        };
        input.events.push(egui::Event::PointerMoved(pos));
        input.events.push(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: held,
            modifiers: Default::default(),
        });
        if wheel {
            input.events.push(egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, -60.0),
                modifiers: Default::default(),
            });
        }
        let theme = Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0);
        // Shapes are not needed: the assertions read the actual stored ScrollArea state.
        let _ = self.ctx.run(input, |ctx| {
            if held {
                egui::DragAndDrop::set_payload(
                    ctx,
                    Payload {
                        paths: vec![PathBuf::from("/scroll-fixture/item")],
                        single_dir: false,
                        source: egui::Id::new("source"),
                        origin: None,
                        volume: None,
                    },
                );
            } else {
                egui::DragAndDrop::clear_payload(ctx);
            }
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show(ctx, |ui| {
                    for &(index, top, bottom) in rects {
                        let rect = egui::Rect::from_min_max(
                            egui::pos2(100.0, top),
                            egui::pos2(400.0, bottom),
                        );
                        let mut child =
                            ui.new_child(egui::UiBuilder::new().id_salt(index).max_rect(rect));
                        child.set_clip_rect(rect);
                        let mut table_output = None;
                        let output = egui::ScrollArea::vertical()
                            .id_salt(index)
                            .auto_shrink([false, false])
                            .drag_to_scroll(false)
                            .show(&mut child, |ui| {
                                if self.table {
                                    use tasty_ui_widgets::{
                                        Table, TableAlign, TableColumn, TableColumnWidth,
                                    };
                                    table_output = Some(
                                        Table::new(vec![TableColumn {
                                            title: "Name",
                                            width: TableColumnWidth::Exact(LogicalPx(200.0)),
                                            align: TableAlign::Left,
                                            sort_id: None::<usize>,
                                        }])
                                        .virtual_rows(true)
                                        .show(
                                            ui,
                                            &theme,
                                            &[(); 120],
                                            |_| false,
                                            |ui, _, _, _| {
                                                ui.label("row");
                                            },
                                        )
                                        .scroll,
                                    );
                                } else {
                                    ui.allocate_space(egui::vec2(200.0, 3000.0));
                                }
                            });
                        let output = table_output.unwrap_or(output);
                        self.ids[index] = Some(output.id);
                        super::super::drag_scroll::feed(
                            &child,
                            &theme,
                            &DragState::default(),
                            &output,
                            LogicalPx(28.0),
                        );
                    }
                });
        });
        self.frame += 1;
        self.ids.map(|id| {
            id.and_then(|id| egui::scroll_area::State::load(&self.ctx, id))
                .map_or(0.0, |state| state.offset.y)
        })
    }
}

#[test]
fn resizing_held_drag_scrolls_only_the_current_lower_viewport() {
    for (table, reversed) in [(false, false), (false, true), (true, false), (true, true)] {
        let mut harness = Harness::new(table);
        let mut tall = vec![(0, 100.0, 900.0), (1, 950.0, 1750.0)];
        let mut short = vec![(0, 100.0, 300.0), (1, 350.0, 550.0)];
        if reversed {
            tall.reverse();
            short.reverse();
        }
        assert_eq!(harness.frame(&tall, true, false), [0.0, 0.0]);
        let resized = harness.frame(&short, true, false);
        assert_eq!(resized[0], 0.0);
        let expected = 28.0 * 20.0 / 60.0 * (14.0_f32 / 24.0).powi(2);
        assert!((resized[1] - expected).abs() < 0.001, "{resized:?}");
        let stable = harness.frame(&short, true, false);
        assert_eq!(stable[0], 0.0);
        assert!((stable[1] - 2.0 * expected).abs() < 0.001);
    }
}

#[test]
fn removed_viewport_cannot_own_or_receive_the_next_scroll() {
    let mut harness = Harness::new(true);
    let initial = harness.frame(&[(0, 100.0, 300.0), (1, 350.0, 550.0)], true, false);
    assert!(initial[1] > 0.0);
    let removed = harness.frame(&[(0, 100.0, 550.0)], true, false);
    assert!(removed[0] > 0.0, "the surviving viewport must become owner");
    assert_eq!(removed[1], initial[1]);
    assert_eq!(harness.frame(&[], true, false), removed);
}

#[test]
fn resize_without_drag_does_not_scroll_and_wheel_still_works() {
    let mut harness = Harness::new(true);
    assert_eq!(
        harness.frame(&[(0, 100.0, 900.0), (1, 950.0, 1750.0)], false, false),
        [0.0, 0.0]
    );
    let short = [(0, 100.0, 300.0), (1, 350.0, 550.0)];
    assert_eq!(harness.frame(&short, false, false), [0.0, 0.0]);
    let wheel = harness.frame(&short, false, true);
    assert_eq!(wheel[0], 0.0);
    assert!(wheel[1] > 0.0);
}
