use crate::adapters::ui::surface::image::view::ImageView;
use crate::i18n::t;
use crate::model::ImagePanel;
use crate::theme;
use egui::emath::GuiRounding as _;

pub(super) fn draw_viewer_controls(
    ui: &mut egui::Ui,
    panel: &mut ImagePanel,
    view: &mut ImageView,
    th: &theme::Theme,
) {
    let has_dir = !panel.dir_images.is_empty();

    if has_dir {
        if ui
            .add_sized([24.0, 20.0], egui::Button::new("\u{25C0}"))
            .on_hover_text(t("image_viewer.prev"))
            .clicked()
            && !view.is_editing()
            && panel.step_prev().is_some()
        {
            view.load_after_navigation(panel);
        }
        if ui
            .add_sized([24.0, 20.0], egui::Button::new("\u{25B6}"))
            .on_hover_text(t("image_viewer.next"))
            .clicked()
            && !view.is_editing()
            && panel.step_next().is_some()
        {
            view.load_after_navigation(panel);
        }
    }

    if ui
        .add_sized([24.0, 20.0], egui::Button::new("\u{21BB}"))
        .on_hover_text(t("image_viewer.refresh"))
        .clicked()
    {
        view.reload_from_disk(panel);
    }

    if view.original_image.is_some()
        && ui
            .add_sized([24.0, 20.0], egui::Button::new("\u{270F}"))
            .on_hover_text(t("image_viewer.edit"))
            .clicked()
    {
        view.enter_edit_mode();
    }

    if ui
        .add_sized([24.0, 20.0], egui::Button::new("+"))
        .on_hover_text(t("image_viewer.new_image"))
        .clicked()
    {
        view.new_image_popup = true;
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
        draw_zoom_controls(ui, view, th);
    });
}

pub(super) fn draw_edit_controls(
    ui: &mut egui::Ui,
    panel: &mut ImagePanel,
    view: &mut ImageView,
    th: &theme::Theme,
) {
    if ui
        .add_sized([50.0, 20.0], egui::Button::new(t("image_viewer.save")))
        .clicked()
    {
        if let Some(path) = panel.save_path() {
            if let Err(e) = view.save_png(&path) {
                tracing::warn!("Failed to save image: {}", e);
            } else {
                view.exit_edit_mode();
                view.reload_from_disk(panel);
            }
        } else {
            // New image — need path input
            view.save_path_popup = true;
        }
    }

    if ui
        .add_sized([50.0, 20.0], egui::Button::new(t("image_viewer.cancel")))
        .clicked()
    {
        view.exit_edit_mode();
    }

    // Undo button
    let undo_enabled = view.can_undo();
    if ui
        .add_enabled(
            undo_enabled,
            egui::Button::new("↶").min_size(egui::vec2(24.0, 20.0)),
        )
        .on_hover_text(t("image_viewer.undo"))
        .clicked()
    {
        view.undo();
    }

    // Redo button
    let redo_enabled = view.can_redo();
    if ui
        .add_enabled(
            redo_enabled,
            egui::Button::new("↷").min_size(egui::vec2(24.0, 20.0)),
        )
        .on_hover_text(t("image_viewer.redo"))
        .clicked()
    {
        view.redo();
    }

    ui.separator();

    // Brush size
    ui.label(
        egui::RichText::new(t("image_viewer.brush_size"))
            .size(th.font_size_caption.value())
            .color(th.subtext0),
    );
    ui.add(egui::Slider::new(&mut view.brush_size, 1.0..=20.0).show_value(false));

    // Color picker
    ui.label(
        egui::RichText::new(t("image_viewer.color"))
            .size(th.font_size_caption.value())
            .color(th.subtext0),
    );
    let mut color_arr = [
        view.brush_color.r(),
        view.brush_color.g(),
        view.brush_color.b(),
    ];
    if ui.color_edit_button_srgb(&mut color_arr).changed() {
        // 사용자 입력 (브러시 색 picker). 정당한 dangerously 사용처.
        #[allow(clippy::disallowed_methods)]
        let new_color = egui::Color32::from_rgb(color_arr[0], color_arr[1], color_arr[2]);
        view.brush_color = new_color;
    }

    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        draw_zoom_controls(ui, view, th);
    });
}

pub(super) fn draw_zoom_controls(ui: &mut egui::Ui, view: &mut ImageView, th: &theme::Theme) {
    if ui
        .add_sized([30.0, 20.0], egui::Button::new("Fit"))
        .clicked()
    {
        view.zoom = 1.0;
        view.pan_offset = egui::Vec2::ZERO;
    }

    if ui.add_sized([20.0, 20.0], egui::Button::new("+")).clicked() {
        view.zoom = (view.zoom * 1.25).min(20.0);
    }

    let zoom_pct = format!("{}%", (view.zoom * 100.0).round_ui() as i32);
    ui.label(
        egui::RichText::new(zoom_pct)
            .size(th.font_size_caption.value())
            .color(th.subtext0),
    );

    if ui.add_sized([20.0, 20.0], egui::Button::new("-")).clicked() {
        view.zoom = (view.zoom / 1.25).max(0.1);
    }
}
