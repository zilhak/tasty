//! The real frame must derive row geometry from the list, not the adjacent preview.
use super::super::cursor::Step;
use super::*;

fn render(
    list: Option<egui::Rect>,
    hover: Option<egui::Pos2>,
) -> (egui::FullOutput, ExplorerView, Theme) {
    let ctx = egui::Context::default();
    let theme = Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0);
    let body = egui::Rect::from_min_max(egui::pos2(100.0, 40.0), egui::pos2(1100.0, 400.0));
    let row = egui::Rect::from_min_max(egui::pos2(120.0, 60.0), egui::pos2(260.0, 88.0));
    let entry = DirEntryInfo {
        path: PathBuf::from("/owned-preview-fixture/folder"),
        name: "folder".into(),
        is_dir: true,
        ..Default::default()
    };
    let mut view = ExplorerView::new();
    view.set_entries(vec![entry.clone()]);
    view.list_rect = list;
    view.move_cursor(Step::Home, false);
    if let Some(pos) = hover {
        view.ops.drag.os = Some(OsHover {
            paths: vec![PathBuf::from("/owned-preview-fixture/file")],
            pos,
        });
    }
    let output = ctx.run(
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
                    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(body));
                    child.set_clip_rect(body);
                    push(
                        &child,
                        Spot {
                            path: entry.path.clone(),
                            is_dir: true,
                            rect: row,
                            full: row,
                            kind: Kind::Entry { wide: true },
                        },
                    );
                    frame(&child, &theme, &mut view, body, &mut None);
                });
        },
    );
    (output, view, theme)
}

#[test]
fn current_ring_geometry_and_paint_clip_stop_before_preview() {
    for right in [640.0, 812.0, 900.0] {
        let list = egui::Rect::from_min_max(egui::pos2(100.0, 40.0), egui::pos2(right, 400.0));
        let (output, view, theme) = render(Some(list), None);
        let rings: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Rect(rect)
                    if rect.stroke.color == theme.explorer_cursor_ring().to_egui() =>
                {
                    Some((rect.rect, shape.clip_rect))
                }
                _ => None,
            })
            .collect();
        assert_eq!(rings.len(), 1, "actual current-item ring must be drawn");
        let (ring, clip) = rings[0];
        assert_eq!(
            ring.x_range(),
            list.x_range(),
            "ring geometry must end at the list edge, including its right stroke"
        );
        assert!(
            clip.right() <= list.right(),
            "painting must exclude preview"
        );
        assert!(
            view.selected
                .contains(Path::new("/owned-preview-fixture/folder"))
        );
    }
}

#[test]
fn os_drop_over_preview_does_not_claim_a_detail_row_or_list_background() {
    let list = egui::Rect::from_min_max(egui::pos2(100.0, 40.0), egui::pos2(812.0, 400.0));
    for y in [70.0, 300.0] {
        let (_, view, _) = render(Some(list), Some(egui::pos2(950.0, y)));
        assert!(
            view.ops.drag.os_target.is_none(),
            "preview cannot extend a row or the list drop target"
        );
    }
}

#[test]
fn a_list_without_an_adjacent_preview_keeps_full_width() {
    let list = egui::Rect::from_min_max(egui::pos2(100.0, 40.0), egui::pos2(1100.0, 400.0));
    let (output, _, theme) = render(Some(list), None);
    assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Rect(rect) if rect.stroke.color == theme.explorer_cursor_ring().to_egui() && rect.rect.right() == list.right())));
}
