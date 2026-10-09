use super::*;

fn theme() -> Theme {
    Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0)
}

/// 편집 줄을 두 프레임 그려 (줄, 입력 칸) rect 를 돌려준다.
fn rects(layout: ExplorerNameLayout) -> (egui::Rect, egui::Rect) {
    let theme = theme();
    let ctx = egui::Context::default();
    let mut edit = ExplorerNameEdit::new("New folder".into(), 0..10);
    let mut out = None;
    for _ in 0..2 {
        drop(ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show(ctx, |ui| {
                    ui.set_width(400.0);
                    let (row, field, _) = explorer_name_row(
                        ui,
                        &theme,
                        layout,
                        tasty_icons::FOLDER,
                        &mut edit,
                        false,
                    );
                    out = Some((row, field));
                });
        }));
    }
    out.expect("drawn")
}

#[test]
fn a_detail_editor_is_one_table_row_and_stays_in_the_name_column() {
    let theme = theme();
    let (row, field) = rects(ExplorerNameLayout::Detail {
        inset: 10.0,
        name_width: 200.0,
    });
    assert_eq!(row.height(), theme.table_cell_height().value());
    assert!(field.right() <= row.left() + 200.0, "{field:?}");
}

#[test]
fn a_list_editor_grows_the_row_to_the_table_row_height() {
    let theme = theme();
    let (row, _) = rects(ExplorerNameLayout::List);
    assert!(theme.table_cell_height().value() > theme.tree_row_height().value());
    assert_eq!(row.height(), theme.table_cell_height().value());
}

#[test]
fn a_grid_editor_centres_a_field_wider_than_the_cell() {
    let theme = theme();
    let cell = egui::vec2(80.0, 100.0);
    let (rect, field) = rects(ExplorerNameLayout::Grid { cell, slot: 16.0 });
    assert_eq!(rect.size(), cell);
    assert_eq!(field.width(), theme.field_width_md.value());
    assert!((field.center().x - rect.center().x).abs() < 0.5);
}

#[test]
fn the_first_frame_takes_the_initial_selection() {
    let theme = theme();
    let ctx = egui::Context::default();
    let mut edit = ExplorerNameEdit::new("untitled.txt".into(), 0..8);
    let mut field_id = None;
    for _ in 0..3 {
        drop(ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                explorer_name_row(
                    ui,
                    &theme,
                    ExplorerNameLayout::List,
                    tasty_icons::FILE,
                    &mut edit,
                    false,
                );
                field_id = ctx.memory(|m| m.focused());
            });
        }));
    }
    assert!(edit.initial_selection.is_none());
    let id = field_id.expect("the field has focus");
    let state = egui::TextEdit::load_state(&ctx, id).expect("state");
    let range = state.cursor.char_range().expect("range");
    assert_eq!(
        (
            range.primary.index.min(range.secondary.index),
            range.primary.index.max(range.secondary.index)
        ),
        (0, 8)
    );
}
