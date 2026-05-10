//! `PluginsWindow` modal의 egui UI.
//!
//! 좌측 — 설치된 plugin 목록.
//! 우측 — 선택된 plugin 상세: 매니페스트, enable/disable, 권한 grant/revoke, uninstall.
//!
//! 모달은 `PluginsSnapshot`(읽기 전용 데이터)을 들고 있고, 사용자 조작은
//! `PluginsAction` 큐에 쌓여 메인 루프에서 `PluginManager`에 적용된다.

use crate::i18n::t;
use crate::theme;

/// 한 plugin의 화면 표시용 스냅샷.
#[derive(Debug, Clone)]
pub struct PluginEntry {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub authors: Vec<String>,
    pub homepage: String,
    pub enabled: bool,
    pub running: bool,
    pub builtin: bool,
    pub surface_kinds: Vec<String>,
    pub manifest_permissions: Vec<String>,
    pub granted_permissions: Vec<String>,
    pub log_path: String,
}

#[derive(Debug, Clone, Default)]
pub struct PluginsSnapshot {
    pub plugins: Vec<PluginEntry>,
}

/// `PluginsWindow`가 메인 루프에 발행하는 동작.
#[derive(Debug, Clone)]
pub enum PluginsAction {
    SetEnabled { id: String, enabled: bool },
    Grant { id: String, permission: String },
    Revoke { id: String, permission: String },
    Uninstall { id: String },
}

/// 모달 자체 상태 (선택, 검색 입력 등).
#[derive(Debug, Default)]
pub struct PluginsUiState {
    pub selected_id: Option<String>,
    pub confirm_uninstall_id: Option<String>,
}

/// modal 메인 그리기. snapshot은 읽기 전용, action은 큐에 추가.
pub fn draw_plugins_panel(
    ctx: &egui::Context,
    snapshot: &PluginsSnapshot,
    ui_state: &mut PluginsUiState,
    actions: &mut Vec<PluginsAction>,
) {
    let th = theme::theme();

    if ui_state.selected_id.is_none() {
        ui_state.selected_id = snapshot.plugins.first().map(|p| p.id.clone());
    } else if let Some(id) = &ui_state.selected_id {
        if !snapshot.plugins.iter().any(|p| &p.id == id) {
            ui_state.selected_id = snapshot.plugins.first().map(|p| p.id.clone());
        }
    }

    egui::TopBottomPanel::top("plugins_header")
        .exact_height(40.0)
        .show(ctx, |ui| {
            ui.horizontal_centered(|ui| {
                ui.add_space(12.0);
                ui.label(
                    egui::RichText::new(t("plugins.title"))
                        .size(14.0)
                        .color(egui::Color32::from(th.text)),
                );
            });
        });

    egui::SidePanel::left("plugins_list")
        .exact_width(240.0)
        .resizable(false)
        .show(ctx, |ui| {
            ui.add_space(8.0);
            if snapshot.plugins.is_empty() {
                ui.add_space(16.0);
                ui.label(
                    egui::RichText::new(t("plugins.empty"))
                        .color(egui::Color32::from(th.subtext0)),
                );
                return;
            }
            egui::ScrollArea::vertical().show(ui, |ui| {
                for entry in &snapshot.plugins {
                    let selected = ui_state.selected_id.as_ref() == Some(&entry.id);
                    let label = if entry.builtin {
                        format!("{}  •", entry.name)
                    } else {
                        entry.name.clone()
                    };
                    let resp = ui.add_sized(
                        egui::vec2(ui.available_width(), 28.0),
                        egui::SelectableLabel::new(selected, label),
                    );
                    if resp.clicked() {
                        ui_state.selected_id = Some(entry.id.clone());
                    }
                    let mut sub = format!("v{}", entry.version);
                    if !entry.enabled {
                        sub.push_str(&format!("  ·  {}", t("plugins.disabled")));
                    } else if entry.running {
                        sub.push_str(&format!("  ·  {}", t("plugins.running")));
                    }
                    ui.label(
                        egui::RichText::new(sub)
                            .small()
                            .color(egui::Color32::from(th.subtext0)),
                    );
                    ui.add_space(4.0);
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
            ui.add_space(6.0);

            if !entry.description.is_empty() {
                ui.label(&entry.description);
                ui.add_space(6.0);
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

            ui.add_space(10.0);
            ui.separator();

            ui.add_space(10.0);
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

            ui.add_space(10.0);
            ui.separator();
            ui.add_space(10.0);
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

            ui.add_space(10.0);
            ui.separator();
            ui.add_space(10.0);
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
                ui.label(
                    egui::RichText::new(t(warn_key))
                        .color(egui::Color32::from(th.peach)),
                );
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
