use egui::emath::GuiRounding as _;

use crate::i18n::t;
use crate::model::ImagePanel;
use crate::theme;

/// Render the image viewer/editor panel.
pub fn draw_image(ui: &mut egui::Ui, panel: &mut ImagePanel) {
    let th = theme::theme();

    // ── New image popup ──
    if panel.new_image_popup {
        draw_new_image_popup(ui, panel, &th);
        return;
    }

    // ── Save path popup ──
    if panel.save_path_popup {
        draw_save_path_popup(ui, panel, &th);
        return;
    }

    // ── Control bar ──
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = th.spacing_sm.value();

        if panel.is_editing() {
            draw_edit_controls(ui, panel, &th);
        } else {
            draw_viewer_controls(ui, panel, &th);
        }
    });

    ui.add_space(2.0);

    // ── Image area ──
    let available = ui.available_rect_before_wrap();
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(available.width(), available.height()),
        egui::Sense::click_and_drag(),
    );

    // Fill background
    ui.painter().rect_filled(rect, 0.0, th.mantle);

    if let Some(ref img) = panel.original_image {
        let [img_w, img_h] = img.size;

        // Upload texture if needed
        if panel.texture.is_none() {
            panel.texture = Some(ui.ctx().load_texture(
                format!("image_panel_{}", panel.id),
                img.clone(),
                egui::TextureOptions::LINEAR,
            ));
        }

        // Upload draw layer texture if needed
        if panel.is_editing() {
            if let Some(ref layer) = panel.draw_layer {
                if panel.draw_texture.is_none() || panel.draw_texture_dirty {
                    panel.draw_texture = Some(ui.ctx().load_texture(
                        format!("image_draw_{}", panel.id),
                        layer.clone(),
                        egui::TextureOptions::LINEAR,
                    ));
                    panel.draw_texture_dirty = false;
                }
            }
        }

        // Compute display size with zoom
        let zoom = panel.zoom;
        let display_w = img_w as f32 * zoom;
        let display_h = img_h as f32 * zoom;

        // Fit to window if zoom <= 1.0
        let (final_w, final_h, effective_zoom) = if zoom <= 1.0 {
            let scale_x = rect.width() / img_w as f32;
            let scale_y = rect.height() / img_h as f32;
            let fit = scale_x.min(scale_y).min(1.0);
            (img_w as f32 * fit, img_h as f32 * fit, fit)
        } else {
            (display_w, display_h, zoom)
        };

        // Center with pan offset
        let center = rect.center() + panel.pan_offset;
        let img_rect = egui::Rect::from_center_size(center, egui::vec2(final_w, final_h));

        // Draw the original image
        if let Some(ref tex) = panel.texture {
            let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
            ui.painter()
                .image(tex.id(), img_rect, uv, egui::Color32::WHITE);
        }

        // Draw the overlay layer
        if panel.is_editing() {
            if let Some(ref tex) = panel.draw_texture {
                let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
                ui.painter()
                    .image(tex.id(), img_rect, uv, egui::Color32::WHITE);
            }
        }

        // ── Mouse interactions ──

        // Zoom with mouse wheel
        let scroll_delta = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll_delta != 0.0
            && rect.contains(ui.input(|i| i.pointer.latest_pos().unwrap_or_default()))
        {
            let factor = if scroll_delta > 0.0 { 1.1 } else { 1.0 / 1.1 };
            panel.zoom = (panel.zoom * factor).clamp(0.1, 20.0);
        }

        // Double click to reset zoom
        if response.double_clicked() {
            panel.zoom = 1.0;
            panel.pan_offset = egui::Vec2::ZERO;
        }

        // Handle Esc key for FloatingSelection cancel
        let esc_pressed = ui.input(|i| i.key_pressed(egui::Key::Escape));
        if esc_pressed && matches!(panel.edit_state, crate::model::EditState::FloatingSelection { .. }) {
            panel.cancel_floating();
        }

        if matches!(panel.edit_state, crate::model::EditState::FloatingSelection { .. }) {
            // FloatingSelection mode
            draw_floating_selection(ui, panel, img_rect, effective_zoom, &response);
        } else if panel.is_editing() {
            // Drawing mode: drag to draw
            if response.dragged() {
                if let Some(pos) = response.interact_pointer_pos() {
                    // Convert screen position to image coordinates
                    let img_x = (pos.x - img_rect.min.x) / effective_zoom;
                    let img_y = (pos.y - img_rect.min.y) / effective_zoom;
                    let img_pos = egui::pos2(img_x, img_y);

                    if panel.last_draw_pos.is_none() {
                        panel.start_stroke();
                    }
                    if let Some(last) = panel.last_draw_pos {
                        panel.draw_line(last, img_pos);
                    } else {
                        panel.draw_line(img_pos, img_pos);
                    }
                    panel.last_draw_pos = Some(img_pos);
                }
            } else {
                if panel.last_draw_pos.is_some() {
                    panel.finish_stroke();
                }
                panel.last_draw_pos = None;
            }
        } else {
            // Viewer mode: drag to pan (only when zoomed)
            if response.dragged() && panel.zoom > 1.0 {
                panel.pan_offset += response.drag_delta();
            }
        }
    } else {
        // No image loaded
        let text = t("image_viewer.no_image");
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            text,
            egui::FontId::proportional(th.font_size_body.value()),
            th.subtext0.into(),
        );
    }
}

fn draw_viewer_controls(ui: &mut egui::Ui, panel: &mut ImagePanel, th: &theme::Theme) {
    let has_dir = !panel.dir_images.is_empty();

    if has_dir {
        if ui
            .add_sized([24.0, 20.0], egui::Button::new("\u{25C0}"))
            .on_hover_text(t("image_viewer.prev"))
            .clicked()
        {
            panel.prev_image();
        }
        if ui
            .add_sized([24.0, 20.0], egui::Button::new("\u{25B6}"))
            .on_hover_text(t("image_viewer.next"))
            .clicked()
        {
            panel.next_image();
        }
    }

    if ui
        .add_sized([24.0, 20.0], egui::Button::new("\u{21BB}"))
        .on_hover_text(t("image_viewer.refresh"))
        .clicked()
    {
        panel.reload();
    }

    if panel.original_image.is_some() {
        if ui
            .add_sized([24.0, 20.0], egui::Button::new("\u{270F}"))
            .on_hover_text(t("image_viewer.edit"))
            .clicked()
        {
            panel.enter_edit_mode();
        }
    }

    if ui
        .add_sized([24.0, 20.0], egui::Button::new("+"))
        .on_hover_text(t("image_viewer.new_image"))
        .clicked()
    {
        panel.new_image_popup = true;
    }

    ui.add_space(8.0);

    // File info
    if let Some(ref path) = panel.file_path {
        let name = std::path::Path::new(path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let info = if panel.dir_images.len() > 1 {
            format!(
                "{} ({}/{})",
                name,
                panel.current_index + 1,
                panel.dir_images.len()
            )
        } else {
            name
        };
        ui.label(
            egui::RichText::new(info)
                .size(th.font_size_caption.value())
                .color(th.subtext0),
        );
    }

    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        draw_zoom_controls(ui, panel, th);
    });
}

fn draw_edit_controls(ui: &mut egui::Ui, panel: &mut ImagePanel, th: &theme::Theme) {
    if ui
        .add_sized([50.0, 20.0], egui::Button::new(t("image_viewer.save")))
        .clicked()
    {
        if let Some(path) = panel.save_path() {
            if let Err(e) = panel.save_png(&path) {
                tracing::warn!("Failed to save image: {}", e);
            } else {
                panel.exit_edit_mode();
                panel.reload();
            }
        } else {
            // New image — need path input
            panel.save_path_popup = true;
        }
    }

    if ui
        .add_sized([50.0, 20.0], egui::Button::new(t("image_viewer.cancel")))
        .clicked()
    {
        panel.exit_edit_mode();
    }

    // Undo button
    let undo_enabled = panel.can_undo();
    if ui
        .add_enabled(undo_enabled, egui::Button::new("↶").min_size(egui::vec2(24.0, 20.0)))
        .on_hover_text(t("image_viewer.undo"))
        .clicked()
    {
        panel.undo();
    }

    // Redo button
    let redo_enabled = panel.can_redo();
    if ui
        .add_enabled(redo_enabled, egui::Button::new("↷").min_size(egui::vec2(24.0, 20.0)))
        .on_hover_text(t("image_viewer.redo"))
        .clicked()
    {
        panel.redo();
    }

    ui.separator();

    // Brush size
    ui.label(
        egui::RichText::new(t("image_viewer.brush_size"))
            .size(th.font_size_caption.value())
            .color(th.subtext0),
    );
    ui.add(egui::Slider::new(&mut panel.brush_size, 1.0..=20.0).show_value(false));

    // Color picker
    ui.label(
        egui::RichText::new(t("image_viewer.color"))
            .size(th.font_size_caption.value())
            .color(th.subtext0),
    );
    let mut color_arr = [
        panel.brush_color.r(),
        panel.brush_color.g(),
        panel.brush_color.b(),
    ];
    if ui.color_edit_button_srgb(&mut color_arr).changed() {
        panel.brush_color = egui::Color32::from_rgb(color_arr[0], color_arr[1], color_arr[2]);
    }

    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        draw_zoom_controls(ui, panel, th);
    });
}

fn draw_zoom_controls(ui: &mut egui::Ui, panel: &mut ImagePanel, th: &theme::Theme) {
    if ui
        .add_sized([30.0, 20.0], egui::Button::new("Fit"))
        .clicked()
    {
        panel.zoom = 1.0;
        panel.pan_offset = egui::Vec2::ZERO;
    }

    if ui.add_sized([20.0, 20.0], egui::Button::new("+")).clicked() {
        panel.zoom = (panel.zoom * 1.25).min(20.0);
    }

    let zoom_pct = format!("{}%", (panel.zoom * 100.0).round_ui() as i32);
    ui.label(
        egui::RichText::new(zoom_pct)
            .size(th.font_size_caption.value())
            .color(th.subtext0),
    );

    if ui.add_sized([20.0, 20.0], egui::Button::new("-")).clicked() {
        panel.zoom = (panel.zoom / 1.25).max(0.1);
    }
}

fn draw_new_image_popup(ui: &mut egui::Ui, panel: &mut ImagePanel, th: &theme::Theme) {
    ui.vertical_centered(|ui| {
        ui.add_space(20.0);
        ui.label(
            egui::RichText::new(t("image_viewer.new_image_title"))
                .size(th.font_size_heading.value())
                .color(th.text),
        );
        ui.add_space(12.0);

        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(t("image_viewer.width"))
                    .size(th.font_size_body.value())
                    .color(th.subtext0),
            );
            ui.add_sized(
                [80.0, 22.0],
                egui::TextEdit::singleline(&mut panel.new_image_width)
                    .font(egui::FontId::proportional(th.font_size_body.value())),
            );
            ui.label(
                egui::RichText::new(" x ")
                    .size(th.font_size_body.value())
                    .color(th.subtext0),
            );
            ui.label(
                egui::RichText::new(t("image_viewer.height"))
                    .size(th.font_size_body.value())
                    .color(th.subtext0),
            );
            ui.add_sized(
                [80.0, 22.0],
                egui::TextEdit::singleline(&mut panel.new_image_height)
                    .font(egui::FontId::proportional(th.font_size_body.value())),
            );
        });

        ui.add_space(12.0);
        ui.horizontal(|ui| {
            if ui.button(t("button.ok")).clicked() {
                let w = panel
                    .new_image_width
                    .parse::<usize>()
                    .unwrap_or(800)
                    .clamp(1, 8192);
                let h = panel
                    .new_image_height
                    .parse::<usize>()
                    .unwrap_or(600)
                    .clamp(1, 8192);
                panel.create_blank_canvas(w, h);
            }
            if ui.button(t("button.cancel")).clicked() {
                panel.new_image_popup = false;
            }
        });
    });
}

/// Render a floating selection overlay and handle mouse interactions.
fn draw_floating_selection(
    ui: &mut egui::Ui,
    panel: &mut ImagePanel,
    img_rect: egui::Rect,
    effective_zoom: f32,
    response: &egui::Response,
) {
    use crate::model::{DragState, EditState};
    let th = theme::theme();

    // Extract selection data for rendering (need immutable access first)
    let (sel_screen_rect, has_texture) = if let EditState::FloatingSelection { ref mut selection, .. } =
        panel.edit_state
    {
        // Upload floating selection texture if needed
        if selection.texture.is_none() {
            selection.texture = Some(ui.ctx().load_texture(
                format!("image_float_{}", panel.id),
                selection.image.clone(),
                egui::TextureOptions::LINEAR,
            ));
        }

        // Compute screen-space rect for the floating selection
        let sel_x = img_rect.min.x + selection.position.x * effective_zoom;
        let sel_y = img_rect.min.y + selection.position.y * effective_zoom;
        let sel_w = selection.size[0] as f32 * effective_zoom;
        let sel_h = selection.size[1] as f32 * effective_zoom;
        let sel_rect = egui::Rect::from_min_size(egui::pos2(sel_x, sel_y), egui::vec2(sel_w, sel_h));

        // Draw the floating image
        if let Some(ref tex) = selection.texture {
            let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
            ui.painter().image(tex.id(), sel_rect, uv, egui::Color32::WHITE);
        }

        // Draw dashed border
        let stroke = egui::Stroke::new(1.0, th.blue);
        ui.painter().rect_stroke(sel_rect, 0.0, stroke, egui::StrokeKind::Outside);

        // Draw 8 resize handles
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

    // Mouse interaction
    let handle_size = 6.0;
    let handles = resize_handle_rects(sel_screen_rect, handle_size);

    if response.drag_started() {
        if let Some(pos) = response.interact_pointer_pos() {
            // Check resize handles first
            let mut found_handle = None;
            for (handle, handle_rect) in &handles {
                if handle_rect.contains(pos) {
                    found_handle = Some(*handle);
                    break;
                }
            }

            let should_commit = if let EditState::FloatingSelection { ref mut selection, .. } = panel.edit_state {
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
                panel.commit_floating();
                return;
            }
        }
    } else if response.dragged() {
        if let Some(pos) = response.interact_pointer_pos() {
            if let EditState::FloatingSelection { ref mut selection, .. } = panel.edit_state {
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
                        // Simple resize: adjust size based on bottom-right drag direction
                        let new_w = ((initial_rect.width() + delta.x).max(10.0) / effective_zoom) as usize;
                        let new_h = ((initial_rect.height() + delta.y).max(10.0) / effective_zoom) as usize;
                        selection.size = [new_w.max(1), new_h.max(1)];
                    }
                    DragState::Idle => {}
                }
            }
        }
    } else if response.drag_stopped() {
        if let EditState::FloatingSelection { ref mut selection, .. } = panel.edit_state {
            selection.drag_state = DragState::Idle;
        }
    } else if response.clicked() {
        // Click without drag
        if let Some(pos) = response.interact_pointer_pos() {
            if !sel_screen_rect.contains(pos) {
                panel.commit_floating();
            }
        }
    }
}

/// Compute screen-space rects for the 8 resize handles around a selection rect.
fn resize_handle_rects(
    sel_rect: egui::Rect,
    handle_size: f32,
) -> Vec<(crate::model::ResizeHandle, egui::Rect)> {
    use crate::model::ResizeHandle;
    let hs = handle_size;
    let mid_x = sel_rect.center().x;
    let mid_y = sel_rect.center().y;
    let l = sel_rect.left();
    let r = sel_rect.right();
    let t = sel_rect.top();
    let b = sel_rect.bottom();

    vec![
        (ResizeHandle::TopLeft, egui::Rect::from_center_size(egui::pos2(l, t), egui::vec2(hs, hs))),
        (ResizeHandle::Top, egui::Rect::from_center_size(egui::pos2(mid_x, t), egui::vec2(hs, hs))),
        (ResizeHandle::TopRight, egui::Rect::from_center_size(egui::pos2(r, t), egui::vec2(hs, hs))),
        (ResizeHandle::Right, egui::Rect::from_center_size(egui::pos2(r, mid_y), egui::vec2(hs, hs))),
        (ResizeHandle::BottomRight, egui::Rect::from_center_size(egui::pos2(r, b), egui::vec2(hs, hs))),
        (ResizeHandle::Bottom, egui::Rect::from_center_size(egui::pos2(mid_x, b), egui::vec2(hs, hs))),
        (ResizeHandle::BottomLeft, egui::Rect::from_center_size(egui::pos2(l, b), egui::vec2(hs, hs))),
        (ResizeHandle::Left, egui::Rect::from_center_size(egui::pos2(l, mid_y), egui::vec2(hs, hs))),
    ]
}

fn draw_save_path_popup(ui: &mut egui::Ui, panel: &mut ImagePanel, th: &theme::Theme) {
    ui.vertical_centered(|ui| {
        ui.add_space(20.0);
        ui.label(
            egui::RichText::new(t("image_viewer.save_path_title"))
                .size(th.font_size_heading.value())
                .color(th.text),
        );
        ui.add_space(12.0);

        ui.horizontal(|ui| {
            let resp = ui.add_sized(
                [ui.available_width() - 40.0, 22.0],
                egui::TextEdit::singleline(&mut panel.save_path_buffer)
                    .font(egui::FontId::proportional(th.font_size_body.value()))
                    .hint_text("path/to/image.png"),
            );
            if !resp.has_focus() && panel.save_path_buffer.is_empty() {
                resp.request_focus();
            }

            if ui
                .add_sized([26.0, 22.0], egui::Button::new("\u{1F4C2}"))
                .clicked()
            {
                let dialog = rfd::FileDialog::new()
                    .add_filter("PNG", &["png"])
                    .set_file_name("image.png");
                if let Some(path) = dialog.save_file() {
                    panel.save_path_buffer = path.to_string_lossy().to_string();
                }
            }
        });

        ui.add_space(12.0);
        ui.horizontal(|ui| {
            if ui.button(t("button.save")).clicked() && !panel.save_path_buffer.is_empty() {
                let mut path = panel.save_path_buffer.clone();
                if !path.ends_with(".png") {
                    path.push_str(".png");
                }
                if let Err(e) = panel.save_png(&path) {
                    tracing::warn!("Failed to save image: {}", e);
                } else {
                    panel.file_path = Some(path);
                    panel.save_path_popup = false;
                    panel.exit_edit_mode();
                }
            }
            if ui.button(t("button.cancel")).clicked() {
                panel.save_path_popup = false;
            }
        });
    });
}
