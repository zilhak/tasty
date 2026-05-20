use crate::i18n::t;
use crate::model::ImagePanel;
use crate::theme;
use crate::ui::surface::image::view::ImageView;

pub(super) fn draw_new_image_popup(ui: &mut egui::Ui, view: &mut ImageView, th: &theme::Theme) {
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
                egui::TextEdit::singleline(&mut view.new_image_width)
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
                egui::TextEdit::singleline(&mut view.new_image_height)
                    .font(egui::FontId::proportional(th.font_size_body.value())),
            );
        });

        ui.add_space(12.0);
        ui.horizontal(|ui| {
            if ui.button(t("button.ok")).clicked() {
                let w = view
                    .new_image_width
                    .parse::<usize>()
                    .unwrap_or(800)
                    .clamp(1, 8192);
                let h = view
                    .new_image_height
                    .parse::<usize>()
                    .unwrap_or(600)
                    .clamp(1, 8192);
                view.create_blank_canvas(w, h);
            }
            if ui.button(t("button.cancel")).clicked() {
                view.new_image_popup = false;
            }
        });
    });
}

pub(super) fn draw_save_path_popup(
    ui: &mut egui::Ui,
    panel: &mut ImagePanel,
    view: &mut ImageView,
    th: &theme::Theme,
) {
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
                egui::TextEdit::singleline(&mut view.save_path_buffer)
                    .font(egui::FontId::proportional(th.font_size_body.value()))
                    .hint_text(crate::theme_bridge::hint_text("path/to/image.png")),
            );
            if !resp.has_focus() && view.save_path_buffer.is_empty() {
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
                    view.save_path_buffer = path.to_string_lossy().to_string();
                }
            }
        });

        ui.add_space(12.0);
        ui.horizontal(|ui| {
            if ui.button(t("button.save")).clicked() && !view.save_path_buffer.is_empty() {
                let mut path = view.save_path_buffer.clone();
                if !path.ends_with(".png") {
                    path.push_str(".png");
                }
                if let Err(e) = view.save_png(&path) {
                    tracing::warn!("Failed to save image: {}", e);
                } else {
                    panel.assign_file_path(path);
                    view.save_path_popup = false;
                    view.exit_edit_mode();
                }
            }
            if ui.button(t("button.cancel")).clicked() {
                view.save_path_popup = false;
            }
        });
    });
}
