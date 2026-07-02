use crate::i18n::t;
use crate::settings::KeybindingSettings;
use tasty_ui_widgets::{hspace, margin_sym, vspace};

pub(super) fn draw_preset_subtab(
    ui: &mut egui::Ui,
    keybindings: &mut KeybindingSettings,
    selected_preset: &mut Option<String>,
) {
    let th = crate::theme::theme();
    vspace(ui, th.spacing_xs);

    ui.horizontal_top(|ui| {
        // 좌측: 프리셋 목록
        egui::Frame::new()
            .fill(th.mantle.into())
            .stroke(egui::Stroke::new(1.0, th.surface0))
            .corner_radius(4.0)
            .inner_margin(margin_sym(th.spacing_sm, th.spacing_sm))
            .show(ui, |ui| {
                ui.set_width(120.0);
                ui.vertical(|ui| {
                    for name in KeybindingSettings::preset_names() {
                        let is_selected = selected_preset.as_deref() == Some(*name);
                        if ui.selectable_label(is_selected, *name).clicked() {
                            *selected_preset = Some(name.to_string());
                        }
                    }
                });
            });

        hspace(ui, th.spacing_sm);

        // 우측: 미리보기 패널
        ui.vertical(|ui| {
            let Some(name) = selected_preset.clone() else {
                ui.label(t("settings.keybindings.select_preset_label"));
                return;
            };
            let Some(preset) = KeybindingSettings::preset_by_name(&name) else {
                ui.label(t("settings.keybindings.select_preset_label"));
                return;
            };

            ui.heading(&name);
            vspace(ui, th.spacing_xs);

            let is_identical = KeybindingSettings::GENERAL_BINDING_FIELDS
                .iter()
                .all(|(id, _)| keybindings.get_bindings(id) == preset.get_bindings(id));

            egui::ScrollArea::vertical()
                .max_height(ui.available_height() - 40.0)
                .auto_shrink([false, false])
                .drag_to_scroll(false)
                .show(ui, |ui| {
                    egui::Grid::new("preset_preview_grid")
                        .num_columns(3)
                        .spacing([16.0, 6.0])
                        .striped(true)
                        .show(ui, |ui| {
                            let strong = |s: String| egui::RichText::new(s).color(th.text).strong();
                            let normal = |s: String| egui::RichText::new(s).color(th.text);

                            ui.label(strong(
                                t("settings.keybindings.preset_col_action").to_string(),
                            ));
                            ui.label(strong(
                                t("settings.keybindings.preset_col_before").to_string(),
                            ));
                            ui.label(strong(
                                t("settings.keybindings.preset_col_after").to_string(),
                            ));
                            ui.end_row();

                            for (field_id, label_key) in KeybindingSettings::GENERAL_BINDING_FIELDS
                            {
                                let before_raw = keybindings.get_bindings(field_id).unwrap_or(&[]);
                                let after_raw = preset.get_bindings(field_id).unwrap_or(&[]);
                                let changed = before_raw != after_raw;

                                let action_label =
                                    t(label_key).trim_end_matches(':').trim().to_string();
                                let fmt_list = |v: &[String]| -> String {
                                    if v.is_empty() {
                                        t("settings.keybindings.hint_none").to_string()
                                    } else {
                                        v.iter()
                                            .map(|b| KeybindingSettings::format_display(b))
                                            .collect::<Vec<_>>()
                                            .join(", ")
                                    }
                                };

                                let make = |s: String| if changed { strong(s) } else { normal(s) };
                                ui.label(make(action_label));
                                ui.label(make(fmt_list(before_raw)));
                                ui.label(make(fmt_list(after_raw)));
                                ui.end_row();
                            }
                        });
                });

            vspace(ui, th.spacing_xs);
            ui.separator();
            vspace(ui, th.spacing_xs);
            ui.horizontal(|ui| {
                let apply_btn = egui::Button::new(t("settings.keybindings.apply_button"));
                if ui.add_enabled(!is_identical, apply_btn).clicked() {
                    keybindings.apply_preset(&name);
                }
            });
        });
    });
}
