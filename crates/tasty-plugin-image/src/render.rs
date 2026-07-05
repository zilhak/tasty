//! egui closure for the image surface — control bar / paint bar chrome + canvas, drawn
//! in the plugin process and tessellated to a mesh the host composites (ADR-0028 / B2).
//!
//! Structure transcribes the design (`gallery/plugins.jsx` Image viewer / the
//! `image_viewer` gallery specimen): a control bar (viewer = ◀ ▶ ↻ ✏ + · filename ·
//! right-aligned zoom; paint = Save / Cancel / ↶ ↷ · brush · color · zoom) over a canvas
//! filled with the sidebar tone. All colors / sizes / spacing come from the `Theme`
//! tokens delivered via `set_context` — no raw px / `from_rgb` hardcoding.

use egui::emath::GuiRounding as _;
use tasty_plugin_sdk::Translator;
use tasty_type_appearance::theme::Theme;

use crate::doc::{DragState, EditState, ImageDoc, ResizeHandle};

/// 빌드타임 SVG 베이크 산출물 (방식 B). `build.rs` 가 `tasty-icons` 의 canonical
/// `<svg>` 를 usvg 로 파싱·평탄화해 `pub const <NAME>: &[&[[f32; 2]]]`(viewBox 0..24
/// 좌표)를 생성한다. 런타임은 이 점배열을 [`tasty_plugin_sdk::baked_icon::draw`] 로
/// 그릴 크기에 스케일해 벡터 stroke 로 그린다(텍스처 없음, DPI 독립).
mod baked_icons {
    include!(concat!(env!("OUT_DIR"), "/plugin_icons.rs"));
}

/// Render one frame of the image surface into `ctx`.
pub fn draw(ctx: &egui::Context, theme: &Theme, tr: &Translator, doc: &mut ImageDoc) {
    let frame = egui::Frame::new().fill(theme.bg_panel().to_egui());
    egui::CentralPanel::default().frame(frame).show(ctx, |ui| {
        // ── Popups take over the whole surface ──
        if doc.new_image_popup {
            draw_new_image_popup(ui, theme, tr, doc);
            return;
        }
        if doc.save_path_popup {
            draw_save_path_popup(ui, theme, tr, doc);
            return;
        }

        let pad = theme.spacing_sm.value();

        // ── Control bar ──
        ui.add_space(pad);
        ui.horizontal(|ui| {
            ui.add_space(pad);
            ui.spacing_mut().item_spacing.x = theme.spacing_xs.value();
            if doc.is_editing() {
                draw_edit_controls(ui, theme, tr, doc);
            } else {
                draw_viewer_controls(ui, theme, tr, doc);
            }
            ui.add_space(pad);
        });
        ui.add_space(pad);

        // ── bar / canvas separator ──
        let sep_y = ui.min_rect().bottom();
        ui.painter().hline(
            ui.max_rect().x_range(),
            sep_y,
            egui::Stroke::new(theme.border_width.value(), theme.separator.to_egui()),
        );

        // ── Canvas area ──
        draw_canvas(ui, theme, tr, doc);
    });
}

// ── Control bar (viewer / paint) ────────────────────────────────────────────

fn draw_viewer_controls(ui: &mut egui::Ui, theme: &Theme, tr: &Translator, doc: &mut ImageDoc) {
    let has_dir = doc.dir_images.len() > 1;

    if has_dir {
        if baked_icon_button(ui, theme, baked_icons::CHEVRON_LEFT, tr.t("image_viewer.prev"))
            .clicked()
            && !doc.is_editing()
            && doc.step_prev().is_some()
        {
            doc.load_after_navigation();
        }
        if baked_icon_button(ui, theme, baked_icons::CHEVRON_RIGHT, tr.t("image_viewer.next"))
            .clicked()
            && !doc.is_editing()
            && doc.step_next().is_some()
        {
            doc.load_after_navigation();
        }
    }

    if baked_icon_button(ui, theme, baked_icons::REFRESH, tr.t("image_viewer.refresh")).clicked() {
        doc.reload_from_disk();
    }

    if doc.original_image.is_some()
        && baked_icon_button(ui, theme, baked_icons::EDIT, tr.t("image_viewer.edit")).clicked()
    {
        doc.enter_edit_mode();
    }

    if baked_icon_button(ui, theme, baked_icons::PLUS, tr.t("image_viewer.new_image")).clicked() {
        doc.new_image_popup = true;
    }

    ui.add_space(theme.spacing_sm.value());

    // File info (name + optional index).
    if let Some(ref path) = doc.file_path {
        let name = std::path::Path::new(path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let info = if doc.dir_images.len() > 1 {
            format!(
                "{} ({}/{})",
                name,
                doc.current_index + 1,
                doc.dir_images.len()
            )
        } else {
            name
        };
        ui.label(caption(theme, &info));
    }

    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        draw_zoom_controls(ui, theme, doc);
    });
}

fn draw_edit_controls(ui: &mut egui::Ui, theme: &Theme, tr: &Translator, doc: &mut ImageDoc) {
    if text_button(ui, theme, tr.t("image_viewer.save")).clicked() {
        if let Some(path) = doc.save_path() {
            if let Err(e) = doc.save_png(&path) {
                tracing::warn!("failed to save image: {e}");
            } else {
                doc.exit_edit_mode();
                doc.reload_from_disk();
            }
        } else {
            // New (blank) image — need a path first.
            doc.save_path_popup = true;
        }
    }

    if text_button(ui, theme, tr.t("image_viewer.cancel")).clicked() {
        doc.exit_edit_mode();
    }

    let undo_enabled = doc.can_undo();
    if baked_icon_button_enabled(
        ui,
        theme,
        baked_icons::UNDO,
        tr.t("image_viewer.undo"),
        undo_enabled,
    )
    .clicked()
    {
        doc.undo();
    }
    let redo_enabled = doc.can_redo();
    if baked_icon_button_enabled(
        ui,
        theme,
        baked_icons::REDO,
        tr.t("image_viewer.redo"),
        redo_enabled,
    )
    .clicked()
    {
        doc.redo();
    }

    ui.separator();

    ui.label(caption(theme, tr.t("image_viewer.brush_size")));
    ui.add(egui::Slider::new(&mut doc.brush_size, 1.0..=20.0).show_value(false));

    ui.label(caption(theme, tr.t("image_viewer.color")));
    let mut color_arr = [
        doc.brush_color.r(),
        doc.brush_color.g(),
        doc.brush_color.b(),
    ];
    if ui.color_edit_button_srgb(&mut color_arr).changed() {
        // 사용자 입력 (브러시 색 picker). 정당한 dangerously 사용처.
        #[allow(clippy::disallowed_methods)]
        let new_color = egui::Color32::from_rgb(color_arr[0], color_arr[1], color_arr[2]);
        doc.brush_color = new_color;
    }

    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        draw_zoom_controls(ui, theme, doc);
    });
}

fn draw_zoom_controls(ui: &mut egui::Ui, theme: &Theme, doc: &mut ImageDoc) {
    // right_to_left layout: add in reverse visual order (-, %, +, Fit).
    if text_button(ui, theme, "-").clicked() {
        doc.zoom = (doc.zoom / 1.25).max(0.1);
    }
    let zoom_pct = format!("{}%", (doc.zoom * 100.0).round_ui() as i32);
    ui.label(caption(theme, &zoom_pct));
    if text_button(ui, theme, "+").clicked() {
        doc.zoom = (doc.zoom * 1.25).min(20.0);
    }
    if text_button(ui, theme, "Fit").clicked() {
        doc.zoom = 1.0;
        doc.pan_offset = egui::Vec2::ZERO;
    }
}

// ── Canvas ──────────────────────────────────────────────────────────────────

fn draw_canvas(ui: &mut egui::Ui, theme: &Theme, tr: &Translator, doc: &mut ImageDoc) {
    let available = ui.available_rect_before_wrap();
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(available.width(), available.height()),
        egui::Sense::click_and_drag(),
    );

    // Canvas background (sidebar tone).
    ui.painter()
        .rect_filled(rect, 0.0, theme.bg_sidebar().to_egui());

    let Some(img) = doc.original_image.as_ref() else {
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            tr.t("image_viewer.no_image"),
            egui::FontId::proportional(theme.font_size_body.value()),
            theme.text_muted().to_egui(),
        );
        return;
    };
    let [img_w, img_h] = img.size;

    // Upload original texture if needed.
    if doc.texture.is_none() {
        let img = doc.original_image.clone().expect("checked above");
        doc.texture = Some(ui.ctx().load_texture(
            "image_original",
            img,
            egui::TextureOptions::LINEAR,
        ));
    }
    // Upload draw-layer texture if needed.
    if doc.is_editing()
        && let Some(layer) = doc.draw_layer.clone()
        && (doc.draw_texture.is_none() || doc.draw_texture_dirty)
    {
        doc.draw_texture = Some(ui.ctx().load_texture(
            "image_draw",
            layer,
            egui::TextureOptions::LINEAR,
        ));
        doc.draw_texture_dirty = false;
    }

    // Compute display size with zoom; fit-to-window when zoom <= 1.0.
    let zoom = doc.zoom;
    let (final_w, final_h, effective_zoom) = if zoom <= 1.0 {
        let scale_x = rect.width() / img_w as f32;
        let scale_y = rect.height() / img_h as f32;
        let fit = scale_x.min(scale_y).min(1.0);
        (img_w as f32 * fit, img_h as f32 * fit, fit)
    } else {
        (img_w as f32 * zoom, img_h as f32 * zoom, zoom)
    };

    let center = rect.center() + doc.pan_offset;
    let img_rect = egui::Rect::from_center_size(center, egui::vec2(final_w, final_h));
    let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));

    if let Some(ref tex) = doc.texture {
        ui.painter()
            .image(tex.id(), img_rect, uv, egui::Color32::WHITE);
    }
    if doc.is_editing()
        && let Some(ref tex) = doc.draw_texture
    {
        ui.painter()
            .image(tex.id(), img_rect, uv, egui::Color32::WHITE);
    }

    // ── Mouse / keyboard interactions ──

    // Zoom with mouse wheel (only over the canvas).
    let scroll_delta = ui.input(|i| i.smooth_scroll_delta.y);
    if scroll_delta != 0.0
        && rect.contains(ui.input(|i| i.pointer.latest_pos().unwrap_or_default()))
    {
        let factor = if scroll_delta > 0.0 { 1.1 } else { 1.0 / 1.1 };
        doc.zoom = (doc.zoom * factor).clamp(0.1, 20.0);
    }

    // Double click resets zoom / pan.
    if response.double_clicked() {
        doc.zoom = 1.0;
        doc.pan_offset = egui::Vec2::ZERO;
    }

    // Esc cancels a floating selection.
    let esc_pressed = ui.input(|i| i.key_pressed(egui::Key::Escape));
    if esc_pressed && matches!(doc.edit_state, EditState::FloatingSelection { .. }) {
        doc.cancel_floating();
    }

    if matches!(doc.edit_state, EditState::FloatingSelection { .. }) {
        draw_floating_selection(ui, theme, doc, img_rect, effective_zoom, &response);
    } else if doc.is_editing() {
        // Drawing mode: drag to draw.
        if response.dragged() {
            if let Some(pos) = response.interact_pointer_pos() {
                let img_x = (pos.x - img_rect.min.x) / effective_zoom;
                let img_y = (pos.y - img_rect.min.y) / effective_zoom;
                let img_pos = egui::pos2(img_x, img_y);
                if doc.last_draw_pos.is_none() {
                    doc.start_stroke();
                }
                if let Some(last) = doc.last_draw_pos {
                    doc.draw_line(last, img_pos);
                } else {
                    doc.draw_line(img_pos, img_pos);
                }
                doc.last_draw_pos = Some(img_pos);
            }
        } else {
            if doc.last_draw_pos.is_some() {
                doc.finish_stroke();
            }
            doc.last_draw_pos = None;
        }
    } else if response.dragged() && doc.zoom > 1.0 {
        // Viewer mode: drag to pan (only when zoomed in).
        doc.pan_offset += response.drag_delta();
    }
}

// ── Floating selection ───────────────────────────────────────────────────────

fn draw_floating_selection(
    ui: &mut egui::Ui,
    theme: &Theme,
    doc: &mut ImageDoc,
    img_rect: egui::Rect,
    effective_zoom: f32,
    response: &egui::Response,
) {
    let handle_size = theme.spacing_sm.value().max(6.0);

    let sel_screen_rect = if let EditState::FloatingSelection {
        ref mut selection, ..
    } = doc.edit_state
    {
        if selection.texture.is_none() {
            selection.texture = Some(ui.ctx().load_texture(
                "image_float",
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

        let stroke =
            egui::Stroke::new(theme.border_width.value(), theme.accent_primary().to_egui());
        ui.painter()
            .rect_stroke(sel_rect, 0.0, stroke, egui::StrokeKind::Outside);

        for (_handle, handle_rect) in resize_handle_rects(sel_rect, handle_size) {
            ui.painter()
                .rect_filled(handle_rect, 0.0, theme.accent_primary().to_egui());
        }
        sel_rect
    } else {
        return;
    };

    let handles = resize_handle_rects(sel_screen_rect, handle_size);

    if response.drag_started() {
        if let Some(pos) = response.interact_pointer_pos() {
            let found_handle = handles
                .iter()
                .find(|(_, r)| r.contains(pos))
                .map(|(h, _)| *h);

            let should_commit = if let EditState::FloatingSelection {
                ref mut selection, ..
            } = doc.edit_state
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
                doc.commit_floating();
            }
        }
    } else if response.dragged() {
        if let Some(pos) = response.interact_pointer_pos()
            && let EditState::FloatingSelection {
                ref mut selection, ..
            } = doc.edit_state
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
    } else if response.drag_stopped() {
        if let EditState::FloatingSelection {
            ref mut selection, ..
        } = doc.edit_state
        {
            selection.drag_state = DragState::Idle;
        }
    } else if response.clicked()
        && let Some(pos) = response.interact_pointer_pos()
        && !sel_screen_rect.contains(pos)
    {
        doc.commit_floating();
    }
}

fn resize_handle_rects(sel_rect: egui::Rect, handle_size: f32) -> Vec<(ResizeHandle, egui::Rect)> {
    let hs = handle_size;
    let mid_x = sel_rect.center().x;
    let mid_y = sel_rect.center().y;
    let l = sel_rect.left();
    let r = sel_rect.right();
    let t = sel_rect.top();
    let b = sel_rect.bottom();
    let sq = |x: f32, y: f32| egui::Rect::from_center_size(egui::pos2(x, y), egui::vec2(hs, hs));
    vec![
        (ResizeHandle::TopLeft, sq(l, t)),
        (ResizeHandle::Top, sq(mid_x, t)),
        (ResizeHandle::TopRight, sq(r, t)),
        (ResizeHandle::Right, sq(r, mid_y)),
        (ResizeHandle::BottomRight, sq(r, b)),
        (ResizeHandle::Bottom, sq(mid_x, b)),
        (ResizeHandle::BottomLeft, sq(l, b)),
        (ResizeHandle::Left, sq(l, mid_y)),
    ]
}

// ── Popups ────────────────────────────────────────────────────────────────────

fn draw_new_image_popup(ui: &mut egui::Ui, theme: &Theme, tr: &Translator, doc: &mut ImageDoc) {
    ui.vertical_centered(|ui| {
        ui.add_space(theme.spacing_lg.value());
        ui.label(heading(theme, tr.t("image_viewer.new_image_title")));
        ui.add_space(theme.spacing_md.value());

        ui.horizontal(|ui| {
            ui.label(body(theme, tr.t("image_viewer.width")));
            ui.add(
                egui::TextEdit::singleline(&mut doc.new_image_width)
                    .desired_width(theme.spacing_lg.value() * 5.0)
                    .font(egui::FontId::proportional(theme.font_size_body.value())),
            );
            ui.label(body(theme, " x "));
            ui.label(body(theme, tr.t("image_viewer.height")));
            ui.add(
                egui::TextEdit::singleline(&mut doc.new_image_height)
                    .desired_width(theme.spacing_lg.value() * 5.0)
                    .font(egui::FontId::proportional(theme.font_size_body.value())),
            );
        });

        ui.add_space(theme.spacing_md.value());
        ui.horizontal(|ui| {
            if text_button(ui, theme, tr.t("button.ok")).clicked() {
                let w = doc
                    .new_image_width
                    .parse::<usize>()
                    .unwrap_or(800)
                    .clamp(1, 8192);
                let h = doc
                    .new_image_height
                    .parse::<usize>()
                    .unwrap_or(600)
                    .clamp(1, 8192);
                doc.create_blank_canvas(w, h);
            }
            if text_button(ui, theme, tr.t("button.cancel")).clicked() {
                doc.new_image_popup = false;
            }
        });
    });
}

fn draw_save_path_popup(ui: &mut egui::Ui, theme: &Theme, tr: &Translator, doc: &mut ImageDoc) {
    ui.vertical_centered(|ui| {
        ui.add_space(theme.spacing_lg.value());
        ui.label(heading(theme, tr.t("image_viewer.save_path_title")));
        ui.add_space(theme.spacing_md.value());

        ui.horizontal(|ui| {
            let resp = ui.add(
                egui::TextEdit::singleline(&mut doc.save_path_buffer)
                    .desired_width(ui.available_width() - theme.spacing_lg.value() * 2.0)
                    .font(egui::FontId::proportional(theme.font_size_body.value())),
            );
            if !resp.has_focus() && doc.save_path_buffer.is_empty() {
                resp.request_focus();
            }
            if baked_icon_button(ui, theme, baked_icons::FOLDER_OPEN, tr.t("image_viewer.browse"))
                .clicked()
            {
                let dialog = rfd::FileDialog::new()
                    .add_filter("PNG", &["png"])
                    .set_file_name("image.png");
                if let Some(path) = dialog.save_file() {
                    doc.save_path_buffer = path.to_string_lossy().to_string();
                }
            }
        });

        ui.add_space(theme.spacing_md.value());
        ui.horizontal(|ui| {
            if text_button(ui, theme, tr.t("button.save")).clicked()
                && !doc.save_path_buffer.is_empty()
            {
                let mut path = doc.save_path_buffer.clone();
                if !path.ends_with(".png") {
                    path.push_str(".png");
                }
                if let Err(e) = doc.save_png(&path) {
                    tracing::warn!("failed to save image: {e}");
                } else {
                    doc.file_path = Some(path);
                    doc.save_path_popup = false;
                    doc.exit_edit_mode();
                }
            }
            if text_button(ui, theme, tr.t("button.cancel")).clicked() {
                doc.save_path_popup = false;
            }
        });
    });
}

// ── Themed widget helpers ─────────────────────────────────────────────────────

/// A caption-sized muted label (filename / zoom % / field labels).
fn caption(theme: &Theme, text: &str) -> egui::RichText {
    egui::RichText::new(text)
        .size(theme.font_size_caption.value())
        .color(theme.text_muted().to_egui())
}

fn body(theme: &Theme, text: &str) -> egui::RichText {
    egui::RichText::new(text)
        .size(theme.font_size_body.value())
        .color(theme.text_muted().to_egui())
}

fn heading(theme: &Theme, text: &str) -> egui::RichText {
    egui::RichText::new(text)
        .size(theme.font_size_heading.value())
        .color(theme.text_primary().to_egui())
}

/// Design-token control button: surface-raised fill + 1px border + caption label.
fn styled_button(theme: &Theme, label: &str) -> egui::Button<'static> {
    egui::Button::new(
        egui::RichText::new(label.to_owned())
            .size(theme.font_size_caption.value())
            .color(theme.text_primary().to_egui()),
    )
    .fill(theme.surface_raised().to_egui())
    .stroke(egui::Stroke::new(
        theme.border_width.value(),
        theme.border_default().to_egui(),
    ))
    .corner_radius(theme.corner_radius_sm.value())
}

/// 아이콘 글리프를 버튼 높이 대비 몇 배로 그릴지. 시각 대조(gx10)로 확정할 튜닝값.
const ICON_DRAW_RATIO: f32 = 0.7;

/// 아이콘 버튼 고정 크기(control-bar). host 는 add_sized([24,20]) 를 썼다.
fn icon_button_size(theme: &Theme) -> [f32; 2] {
    let h = theme.spacing_lg.value() + theme.spacing_xs.value(); // ≈ 20
    let w = theme.spacing_lg.value() * 1.5; // ≈ 24
    [w, h]
}

/// 베이크된 벡터 아이콘 버튼. chrome(배경·보더·hover·active)은 `styled_button` 을
/// 재사용하고(빈 라벨), 그 위에 [`tasty_plugin_sdk::baked_icon::draw`] 로 벡터 stroke
/// 아이콘을 겹쳐 그린다. stroke 색은 텍스트 라벨과 동일한 `text_primary` — 하드코딩 없음.
fn baked_icon_button(
    ui: &mut egui::Ui,
    theme: &Theme,
    icon: &[&[[f32; 2]]],
    tooltip: &str,
) -> egui::Response {
    let [w, h] = icon_button_size(theme);
    let resp = ui
        .add_sized([w, h], styled_button(theme, ""))
        .on_hover_text(tooltip);
    tasty_plugin_sdk::baked_icon::draw(
        ui.painter(),
        icon,
        resp.rect.center(),
        h * ICON_DRAW_RATIO,
        theme.text_primary().to_egui(),
    );
    resp
}

/// 비활성화 가능한 베이크 아이콘 버튼 (undo / redo). disabled 면 chrome 은
/// `add_enabled_ui` 로, 아이콘 stroke 는 `text_muted` 로 흐리게 그린다.
fn baked_icon_button_enabled(
    ui: &mut egui::Ui,
    theme: &Theme,
    icon: &[&[[f32; 2]]],
    tooltip: &str,
    enabled: bool,
) -> egui::Response {
    let [w, h] = icon_button_size(theme);
    let color = if enabled {
        theme.text_primary()
    } else {
        theme.text_muted()
    };
    ui.add_enabled_ui(enabled, |ui| {
        let resp = ui
            .add_sized([w, h], styled_button(theme, ""))
            .on_hover_text(tooltip);
        tasty_plugin_sdk::baked_icon::draw(
            ui.painter(),
            icon,
            resp.rect.center(),
            h * ICON_DRAW_RATIO,
            color.to_egui(),
        );
        resp
    })
    .inner
}

/// Text button (Save / Cancel / Fit / zoom +/-, popup buttons). Auto width.
fn text_button(ui: &mut egui::Ui, theme: &Theme, label: &str) -> egui::Response {
    let h = theme.spacing_lg.value() + theme.spacing_xs.value();
    ui.add(styled_button(theme, label).min_size(egui::vec2(0.0, h)))
}
