use crate::model::ImagePanel;
use crate::theme;
use crate::ui::surface::image::view::{DragState, EditState, ImageView, ResizeHandle};

/// Render a floating selection overlay and handle mouse interactions.
pub(super) fn draw_floating_selection(
    ui: &mut egui::Ui,
    panel: &ImagePanel,
    view: &mut ImageView,
    img_rect: egui::Rect,
    effective_zoom: f32,
    response: &egui::Response,
) {
    let th = theme::theme();

    let (sel_screen_rect, has_texture) = if let EditState::FloatingSelection {
        ref mut selection,
        ..
    } = view.edit_state
    {
        if selection.texture.is_none() {
            selection.texture = Some(ui.ctx().load_texture(
                format!("image_float_{}", panel.id),
                selection.image.clone(),
                egui::TextureOptions::LINEAR,
            ));
        }

        let sel_x = img_rect.min.x + selection.position.x * effective_zoom;
        let sel_y = img_rect.min.y + selection.position.y * effective_zoom;
        let sel_w = selection.size[0] as f32 * effective_zoom;
        let sel_h = selection.size[1] as f32 * effective_zoom;
        let sel_rect =
            egui::Rect::from_min_size(egui::pos2(sel_x, sel_y), egui::vec2(sel_w, sel_h));

        if let Some(ref tex) = selection.texture {
            let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
            ui.painter()
                .image(tex.id(), sel_rect, uv, egui::Color32::WHITE);
        }

        let stroke = egui::Stroke::new(1.0, th.blue);
        ui.painter()
            .rect_stroke(sel_rect, 0.0, stroke, egui::StrokeKind::Outside);

        let handle_size = 6.0;
        let handles = resize_handle_rects(sel_rect, handle_size);
        for (_handle, handle_rect) in &handles {
            ui.painter().rect_filled(*handle_rect, 0.0, th.blue);
        }

        (sel_rect, selection.texture.is_some())
    } else {
        return;
    };

    if !has_texture {
        return;
    }

    let handle_size = 6.0;
    let handles = resize_handle_rects(sel_screen_rect, handle_size);

    if response.drag_started() {
        if let Some(pos) = response.interact_pointer_pos() {
            let mut found_handle = None;
            for (handle, handle_rect) in &handles {
                if handle_rect.contains(pos) {
                    found_handle = Some(*handle);
                    break;
                }
            }

            let should_commit = if let EditState::FloatingSelection {
                ref mut selection, ..
            } = view.edit_state
            {
                if let Some(handle) = found_handle {
                    selection.drag_state = DragState::Resizing {
                        handle,
                        drag_start_pos: pos,
                        initial_rect: sel_screen_rect,
                    };
                    false
                } else if sel_screen_rect.contains(pos) {
                    selection.drag_state = DragState::Moving {
                        drag_start_pos: pos,
                        initial_position: selection.position,
                    };
                    false
                } else {
                    true
                }
            } else {
                false
            };
            if should_commit {
                view.commit_floating();
                return;
            }
        }
    } else if response.dragged() {
        if let Some(pos) = response.interact_pointer_pos() {
            if let EditState::FloatingSelection {
                ref mut selection, ..
            } = view.edit_state
            {
                match selection.drag_state.clone() {
                    DragState::Moving {
                        drag_start_pos,
                        initial_position,
                    } => {
                        let delta = pos - drag_start_pos;
                        selection.position = initial_position + delta / effective_zoom;
                    }
                    DragState::Resizing {
                        handle: _,
                        drag_start_pos,
                        initial_rect,
                    } => {
                        let delta = pos - drag_start_pos;
                        let new_w =
                            ((initial_rect.width() + delta.x).max(10.0) / effective_zoom) as usize;
                        let new_h =
                            ((initial_rect.height() + delta.y).max(10.0) / effective_zoom) as usize;
                        selection.size = [new_w.max(1), new_h.max(1)];
                    }
                    DragState::Idle => {}
                }
            }
        }
    } else if response.drag_stopped() {
        if let EditState::FloatingSelection {
            ref mut selection, ..
        } = view.edit_state
        {
            selection.drag_state = DragState::Idle;
        }
    } else if response.clicked() {
        if let Some(pos) = response.interact_pointer_pos() {
            if !sel_screen_rect.contains(pos) {
                view.commit_floating();
            }
        }
    }
}

pub(super) fn resize_handle_rects(
    sel_rect: egui::Rect,
    handle_size: f32,
) -> Vec<(ResizeHandle, egui::Rect)> {
    let hs = handle_size;
    let mid_x = sel_rect.center().x;
    let mid_y = sel_rect.center().y;
    let l = sel_rect.left();
    let r = sel_rect.right();
    let t = sel_rect.top();
    let b = sel_rect.bottom();

    vec![
        (
            ResizeHandle::TopLeft,
            egui::Rect::from_center_size(egui::pos2(l, t), egui::vec2(hs, hs)),
        ),
        (
            ResizeHandle::Top,
            egui::Rect::from_center_size(egui::pos2(mid_x, t), egui::vec2(hs, hs)),
        ),
        (
            ResizeHandle::TopRight,
            egui::Rect::from_center_size(egui::pos2(r, t), egui::vec2(hs, hs)),
        ),
        (
            ResizeHandle::Right,
            egui::Rect::from_center_size(egui::pos2(r, mid_y), egui::vec2(hs, hs)),
        ),
        (
            ResizeHandle::BottomRight,
            egui::Rect::from_center_size(egui::pos2(r, b), egui::vec2(hs, hs)),
        ),
        (
            ResizeHandle::Bottom,
            egui::Rect::from_center_size(egui::pos2(mid_x, b), egui::vec2(hs, hs)),
        ),
        (
            ResizeHandle::BottomLeft,
            egui::Rect::from_center_size(egui::pos2(l, b), egui::vec2(hs, hs)),
        ),
        (
            ResizeHandle::Left,
            egui::Rect::from_center_size(egui::pos2(l, mid_y), egui::vec2(hs, hs)),
        ),
    ]
}
