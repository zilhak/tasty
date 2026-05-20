use crate::i18n::t;
use crate::model::ImagePanel;
use crate::theme;
use crate::ui::image_view::{EditState, ImageView};

/// Render the image viewer/editor panel using model + host view.
pub fn draw_image(ui: &mut egui::Ui, panel: &mut ImagePanel, view: &mut ImageView) {
    let th = theme::theme();

    // ── New image popup ──
    if view.new_image_popup {
        draw_new_image_popup(ui, view, &th);
        return;
    }

    // ── Save path popup ──
    if view.save_path_popup {
        draw_save_path_popup(ui, panel, view, &th);
        return;
    }

    // ── Control bar ──
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = th.spacing_sm.value();

        if view.is_editing() {
            draw_edit_controls(ui, panel, view, &th);
        } else {
            draw_viewer_controls(ui, panel, view, &th);
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

    if let Some(ref img) = view.original_image {
        let [img_w, img_h] = img.size;

        // Upload texture if needed
        if view.texture.is_none() {
            view.texture = Some(ui.ctx().load_texture(
                format!("image_panel_{}", panel.id),
                img.clone(),
                egui::TextureOptions::LINEAR,
            ));
        }

        // Upload draw layer texture if needed
        if view.is_editing() {
            if let Some(ref layer) = view.draw_layer {
                if view.draw_texture.is_none() || view.draw_texture_dirty {
                    view.draw_texture = Some(ui.ctx().load_texture(
                        format!("image_draw_{}", panel.id),
                        layer.clone(),
                        egui::TextureOptions::LINEAR,
                    ));
                    view.draw_texture_dirty = false;
                }
            }
        }

        // Compute display size with zoom
        let zoom = view.zoom;
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
        let center = rect.center() + view.pan_offset;
        let img_rect = egui::Rect::from_center_size(center, egui::vec2(final_w, final_h));

        // Draw the original image
        if let Some(ref tex) = view.texture {
            let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
            ui.painter()
                .image(tex.id(), img_rect, uv, egui::Color32::WHITE);
        }

        // Draw the overlay layer
        if view.is_editing() {
            if let Some(ref tex) = view.draw_texture {
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
            view.zoom = (view.zoom * factor).clamp(0.1, 20.0);
        }

        // Double click to reset zoom
        if response.double_clicked() {
            view.zoom = 1.0;
            view.pan_offset = egui::Vec2::ZERO;
        }

        // Handle Esc key for FloatingSelection cancel
        let esc_pressed = ui.input(|i| i.key_pressed(egui::Key::Escape));
        if esc_pressed && matches!(view.edit_state, EditState::FloatingSelection { .. }) {
            view.cancel_floating();
        }

        if matches!(view.edit_state, EditState::FloatingSelection { .. }) {
            // FloatingSelection mode
            draw_floating_selection(ui, panel, view, img_rect, effective_zoom, &response);
        } else if view.is_editing() {
            // Drawing mode: drag to draw
            if response.dragged() {
                if let Some(pos) = response.interact_pointer_pos() {
                    // Convert screen position to image coordinates
                    let img_x = (pos.x - img_rect.min.x) / effective_zoom;
                    let img_y = (pos.y - img_rect.min.y) / effective_zoom;
                    let img_pos = egui::pos2(img_x, img_y);

                    if view.last_draw_pos.is_none() {
                        view.start_stroke();
                    }
                    if let Some(last) = view.last_draw_pos {
                        view.draw_line(last, img_pos);
                    } else {
                        view.draw_line(img_pos, img_pos);
                    }
                    view.last_draw_pos = Some(img_pos);
                }
            } else {
                if view.last_draw_pos.is_some() {
                    view.finish_stroke();
                }
                view.last_draw_pos = None;
            }
        } else {
            // Viewer mode: drag to pan (only when zoomed)
            if response.dragged() && view.zoom > 1.0 {
                view.pan_offset += response.drag_delta();
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


mod controls;
mod popups;
mod selection;

use controls::*;
use popups::*;
use selection::*;
