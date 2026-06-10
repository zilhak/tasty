use crate::i18n::t;
use crate::theme;

use super::{PluginsAction, PluginsSnapshot, PluginsUiState};

pub(super) fn draw_list_tab(
    ctx: &egui::Context,
    snapshot: &PluginsSnapshot,
    ui_state: &mut PluginsUiState,
    actions: &mut Vec<PluginsAction>,
) {
    let th = theme::theme();

    if ui_state.selected_id.is_none() {
        ui_state.selected_id = snapshot.plugins.first().map(|p| p.id.clone());
    } else if let Some(id) = &ui_state.selected_id
        && !snapshot.plugins.iter().any(|p| &p.id == id)
    {
        ui_state.selected_id = snapshot.plugins.first().map(|p| p.id.clone());
    }

    egui::SidePanel::left("plugins_list")
        .exact_width(240.0)
        .resizable(false)
        .show(ctx, |ui| {
            ui.add_space(8.0);
            if snapshot.plugins.is_empty() {
                ui.add_space(16.0);
                ui.label(
                    egui::RichText::new(t("plugins.empty")).color(egui::Color32::from(th.subtext0)),
                );
                return;
            }
            egui::ScrollArea::vertical().show(ui, |ui| {
                for entry in &snapshot.plugins {
                    let selected = ui_state.selected_id.as_ref() == Some(&entry.id);
                    let name_text = if entry.builtin {
                        format!("{}  •", entry.name)
                    } else {
                        entry.name.clone()
                    };
                    let mut sub = format!("v{}", entry.version);
                    if !entry.enabled {
                        sub.push_str(&format!("  ·  {}", t("plugins.disabled")));
                    } else if entry.running {
                        sub.push_str(&format!("  ·  {}", t("plugins.running")));
                    }

                    // 이름 + 버전 부제를 한 클릭 영역으로 묶기 위해 직접 그린다.
                    // SelectableLabel은 한 줄만 자연스럽게 표현하므로 painter로 selected/hover
                    // 배경과 두 줄 텍스트를 그려 동일한 visual을 재현.
                    let row_h = 40.0;
                    let (rect, resp) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), row_h),
                        egui::Sense::click(),
                    );
                    let visuals = ui.style().interact_selectable(&resp, selected);
                    if selected || resp.hovered() {
                        ui.painter().rect(
                            rect,
                            visuals.corner_radius,
                            visuals.weak_bg_fill,
                            visuals.bg_stroke,
                            egui::StrokeKind::Inside,
                        );
                    }
                    let pad = egui::vec2(8.0, 6.0);
                    let name_pos = rect.min + pad;
                    ui.painter().text(
                        name_pos,
                        egui::Align2::LEFT_TOP,
                        &name_text,
                        egui::FontId::proportional(13.0),
                        visuals.text_color(),
                    );
                    let sub_pos = name_pos + egui::vec2(0.0, 18.0);
                    ui.painter().text(
                        sub_pos,
                        egui::Align2::LEFT_TOP,
                        &sub,
                        egui::FontId::proportional(10.0),
                        egui::Color32::from(th.subtext0),
                    );
                    if resp.clicked() {
                        ui_state.selected_id = Some(entry.id.clone());
                    }
                    ui.add_space(2.0);
                }
            });
        });

    egui::CentralPanel::default().show(ctx, |ui| {
        let selected_entry = ui_state
            .selected_id
            .as_ref()
            .and_then(|id| snapshot.plugins.iter().find(|p| &p.id == id))
            .cloned();
        let Some(entry) = selected_entry else {
            ui.add_space(24.0);
            ui.label(t("plugins.none_selected"));
            return;
        };

        ui.add_space(8.0);
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading(&entry.name);
                ui.label(format!("v{}", entry.version));
                if entry.builtin {
                    ui.label(
                        egui::RichText::new(t("plugins.builtin_badge"))
                            .small()
                            .color(egui::Color32::from(th.mauve)),
                    );
                }
            });
            ui.label(
                egui::RichText::new(&entry.id)
                    .small()
                    .color(egui::Color32::from(th.subtext0)),
            );
            ui.add_space(8.0);

            if !entry.description.is_empty() {
                ui.label(&entry.description);
                ui.add_space(8.0);
            }
            if !entry.authors.is_empty() {
                ui.label(format!(
                    "{}: {}",
                    t("plugins.authors"),
                    entry.authors.join(", ")
                ));
            }
            if !entry.homepage.is_empty() {
                ui.label(format!("{}: {}", t("plugins.homepage"), entry.homepage));
            }

            ui.add_space(12.0);
            ui.separator();

            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui.label(format!("{}:", t("plugins.status")));
                let mut enabled = entry.enabled;
                if ui.checkbox(&mut enabled, t("plugins.enabled")).changed() {
                    actions.push(PluginsAction::SetEnabled {
                        id: entry.id.clone(),
                        enabled,
                    });
                }
            });

            ui.add_space(8.0);
            ui.label(format!("{}:", t("plugins.surface_kinds")));
            if entry.surface_kinds.is_empty() {
                ui.label(t("plugins.none"));
            } else {
                ui.label(entry.surface_kinds.join(", "));
            }

            ui.add_space(12.0);
            ui.separator();
            ui.add_space(12.0);
            ui.label(format!("{}:", t("plugins.permissions")));
            if entry.manifest_permissions.is_empty() {
                ui.label(t("plugins.none"));
            } else {
                for token in &entry.manifest_permissions {
                    let mut granted = entry.granted_permissions.iter().any(|t| t == token);
                    if ui.checkbox(&mut granted, token).changed() {
                        if granted {
                            actions.push(PluginsAction::Grant {
                                id: entry.id.clone(),
                                permission: token.clone(),
                            });
                        } else {
                            actions.push(PluginsAction::Revoke {
                                id: entry.id.clone(),
                                permission: token.clone(),
                            });
                        }
                    }
                }
            }

            ui.add_space(12.0);
            ui.separator();
            ui.add_space(12.0);
            ui.label(format!("{}:", t("plugins.install_path")));
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(&entry.install_dir)
                        .small()
                        .color(egui::Color32::from(th.subtext0)),
                );
                if ui.small_button(t("plugins.open_folder")).clicked() {
                    actions.push(PluginsAction::OpenInstallDir {
                        path: entry.install_dir.clone(),
                    });
                }
            });

            ui.add_space(6.0);
            ui.label(
                egui::RichText::new(format!("{}: {}", t("plugins.log_path"), entry.log_path))
                    .small()
                    .color(egui::Color32::from(th.subtext0)),
            );

            ui.add_space(16.0);
            if ui_state.confirm_uninstall_id.as_ref() == Some(&entry.id) {
                let warn_key = if entry.builtin {
                    "plugins.uninstall_builtin_warning"
                } else {
                    "plugins.uninstall_warning"
                };
                ui.label(egui::RichText::new(t(warn_key)).color(egui::Color32::from(th.peach)));
                ui.horizontal(|ui| {
                    if ui.button(t("plugins.uninstall_confirm")).clicked() {
                        actions.push(PluginsAction::Uninstall {
                            id: entry.id.clone(),
                        });
                        ui_state.confirm_uninstall_id = None;
                    }
                    if ui.button(t("button.cancel")).clicked() {
                        ui_state.confirm_uninstall_id = None;
                    }
                });
            } else if ui.button(t("plugins.uninstall")).clicked() {
                ui_state.confirm_uninstall_id = Some(entry.id.clone());
            }
        });
    });
}
