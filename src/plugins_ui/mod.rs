//! `PluginsWindow` modal의 egui UI.
//!
//! 상단 — 탭 바 (`Installed` / `Add plugin`).
//! `Installed` 탭: 좌측 plugin 목록, 우측 상세(매니페스트, enable/disable,
//! 권한 grant/revoke, 설치 경로, uninstall).
//! `Add plugin` 탭: 경로 입력 → 검증 → 추가/취소.
//!
//! 모달은 `PluginsSnapshot`(읽기 전용 데이터)을 들고 있고, 사용자 조작은
//! `PluginsAction` 큐에 쌓여 메인 루프에서 `PluginManager`에 적용된다.

use crate::i18n::{t, t_fmt};
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
    /// 설치 디렉터리 (`~/.tasty/plugins/<id>/`).
    pub install_dir: String,
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
    /// 설치 디렉터리를 OS 파일 매니저로 연다.
    OpenInstallDir { path: String },
    /// 외부 디렉터리(`src_path`)를 `~/.tasty/plugins/<id>/`로 복사 설치.
    Install { src_path: String },
}

/// 현재 활성 탭.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginsTab {
    List,
    Add,
}

impl Default for PluginsTab {
    fn default() -> Self {
        Self::List
    }
}

/// `Add` 탭에서 사용자가 경로를 검증한 결과 — 추가/취소 확인 단계로 진입.
#[derive(Debug, Clone)]
pub struct AddPreview {
    pub src_path: String,
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub authors: Vec<String>,
    pub homepage: String,
    pub surface_kinds: Vec<String>,
    pub permissions: Vec<String>,
    /// 이미 같은 id의 플러그인이 설치되어 있으면 메시지 — 추가 버튼 비활성화.
    pub already_installed: Option<String>,
}

/// 모달 자체 상태 (탭, 선택, 검색 입력 등).
#[derive(Debug, Default)]
pub struct PluginsUiState {
    pub active_tab: PluginsTab,
    pub selected_id: Option<String>,
    pub confirm_uninstall_id: Option<String>,
    /// `Add` 탭의 경로 입력 버퍼.
    pub add_path_input: String,
    /// 검증 후 preview 정보. 있으면 추가/취소 화면을 보여준다.
    pub add_preview: Option<AddPreview>,
    /// 검증 실패 시 에러 메시지 (UI 하단에 빨간 글씨로 표시).
    pub add_error: Option<String>,
}

/// modal 메인 그리기. snapshot은 읽기 전용, action은 큐에 추가.
pub fn draw_plugins_panel(
    ctx: &egui::Context,
    snapshot: &PluginsSnapshot,
    ui_state: &mut PluginsUiState,
    actions: &mut Vec<PluginsAction>,
) {
    let th = theme::theme();

    egui::TopBottomPanel::top("plugins_header")
        .exact_height(72.0)
        .show(ctx, |ui| {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                ui.label(
                    egui::RichText::new(t("plugins.title"))
                        .size(14.0)
                        .color(egui::Color32::from(th.text)),
                );
            });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                let tabs = [
                    (PluginsTab::List, t("plugins.tab_list")),
                    (PluginsTab::Add, t("plugins.tab_add")),
                ];
                for (tab, label) in &tabs {
                    let selected = ui_state.active_tab == *tab;
                    if ui.selectable_label(selected, *label).clicked() {
                        ui_state.active_tab = *tab;
                    }
                }
            });
            ui.add_space(2.0);
        });

    match ui_state.active_tab {
        PluginsTab::List => draw_list_tab(ctx, snapshot, ui_state, actions),
        PluginsTab::Add => draw_add_tab(ctx, snapshot, ui_state, actions),
    }
}

fn draw_list_tab(
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
            ui.label(format!(
                "{}:",
                t("plugins.install_path")
            ));
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

fn draw_add_tab(
    ctx: &egui::Context,
    snapshot: &PluginsSnapshot,
    ui_state: &mut PluginsUiState,
    actions: &mut Vec<PluginsAction>,
) {
    let th = theme::theme();

    egui::CentralPanel::default().show(ctx, |ui| {
        ui.add_space(12.0);
        if ui_state.add_preview.is_some() {
            draw_add_preview(ui, snapshot, ui_state, actions, &th);
        } else {
            draw_add_input(ui, snapshot, ui_state, &th);
        }
    });
}

/// `Add` 탭의 초기 화면 — 경로 입력 + 확인 + 찾기.
fn draw_add_input(
    ui: &mut egui::Ui,
    snapshot: &PluginsSnapshot,
    ui_state: &mut PluginsUiState,
    th: &theme::Theme,
) {
    ui.label(t("plugins.add_path_label"));
    ui.add_space(6.0);

    let mut submitted = false;
    ui.horizontal(|ui| {
        let edit = egui::TextEdit::singleline(&mut ui_state.add_path_input)
            .hint_text(crate::theme_bridge::hint_text(t(
                "plugins.add_path_placeholder",
            )))
            .desired_width(ui.available_width() - 90.0);
        let resp = ui.add(edit);
        if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            submitted = true;
        }
        if ui.button(t("plugins.add_confirm_path")).clicked() {
            submitted = true;
        }
    });

    if submitted {
        try_validate_path(ui_state, snapshot);
    }

    if let Some(err) = &ui_state.add_error {
        ui.add_space(6.0);
        ui.label(
            egui::RichText::new(err)
                .color(egui::Color32::from(th.red)),
        );
    }

    ui.add_space(20.0);
    ui.separator();
    ui.add_space(12.0);

    if ui.button(t("plugins.add_browse")).clicked() {
        let dialog = rfd::FileDialog::new();
        if let Some(path) = dialog.pick_folder() {
            ui_state.add_path_input = path.to_string_lossy().to_string();
            try_validate_path(ui_state, snapshot);
        }
    }
}

/// 검증된 매니페스트 정보를 보여주고 추가/취소 버튼.
fn draw_add_preview(
    ui: &mut egui::Ui,
    _snapshot: &PluginsSnapshot,
    ui_state: &mut PluginsUiState,
    actions: &mut Vec<PluginsAction>,
    th: &theme::Theme,
) {
    // `take()`는 cancel/add 모두에서 preview를 소비하기 위함이지만, 이 함수가
    // 끝날 때까지 표시할 데이터가 필요하므로 clone 후 다시 넣지 않는다.
    let preview = ui_state.add_preview.clone().expect("checked by caller");

    ui.heading(t("plugins.add_preview_heading"));
    ui.add_space(8.0);
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .max_height(ui.available_height() - 60.0)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(&preview.name)
                        .size(16.0)
                        .color(egui::Color32::from(th.text)),
                );
                ui.label(format!("v{}", preview.version));
            });
            ui.label(
                egui::RichText::new(&preview.id)
                    .small()
                    .color(egui::Color32::from(th.subtext0)),
            );
            ui.add_space(8.0);

            if !preview.description.is_empty() {
                ui.label(&preview.description);
                ui.add_space(6.0);
            }
            if !preview.authors.is_empty() {
                ui.label(format!(
                    "{}: {}",
                    t("plugins.authors"),
                    preview.authors.join(", ")
                ));
            }
            if !preview.homepage.is_empty() {
                ui.label(format!("{}: {}", t("plugins.homepage"), preview.homepage));
            }
            ui.add_space(8.0);

            ui.label(format!(
                "{}: {}",
                t("plugins.add_source_path"),
                preview.src_path
            ));
            ui.add_space(8.0);

            ui.label(format!("{}:", t("plugins.surface_kinds")));
            if preview.surface_kinds.is_empty() {
                ui.label(t("plugins.none"));
            } else {
                ui.label(preview.surface_kinds.join(", "));
            }
            ui.add_space(8.0);

            ui.label(format!("{}:", t("plugins.permissions")));
            if preview.permissions.is_empty() {
                ui.label(t("plugins.none"));
            } else {
                for token in &preview.permissions {
                    ui.label(format!("• {token}"));
                }
            }

            if let Some(msg) = &preview.already_installed {
                ui.add_space(10.0);
                ui.label(
                    egui::RichText::new(msg)
                        .color(egui::Color32::from(th.peach)),
                );
            }
        });

    ui.add_space(10.0);
    ui.separator();
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        let can_add = preview.already_installed.is_none();
        let add_btn = ui.add_enabled(can_add, egui::Button::new(t("plugins.add_button")));
        if add_btn.clicked() {
            actions.push(PluginsAction::Install {
                src_path: preview.src_path.clone(),
            });
            reset_add_state(ui_state);
        }
        if ui.button(t("button.cancel")).clicked() {
            reset_add_state(ui_state);
        }
    });
}

/// `Add` 탭의 상태를 초기 입력 화면으로 되돌린다.
fn reset_add_state(ui_state: &mut PluginsUiState) {
    ui_state.add_preview = None;
    ui_state.add_error = None;
    ui_state.add_path_input.clear();
}

/// 입력 경로로 매니페스트를 로드하고 preview/에러를 채운다.
fn try_validate_path(ui_state: &mut PluginsUiState, snapshot: &PluginsSnapshot) {
    let raw = ui_state.add_path_input.trim().to_string();
    ui_state.add_error = None;
    ui_state.add_preview = None;
    if raw.is_empty() {
        return;
    }
    let path = std::path::PathBuf::from(&raw);
    match crate::plugin::Manifest::load(&path) {
        Ok(manifest) => {
            let already = snapshot
                .plugins
                .iter()
                .any(|p| p.id == manifest.id)
                .then(|| t_fmt("plugins.add_already_installed", &manifest.id));
            ui_state.add_preview = Some(AddPreview {
                src_path: path.to_string_lossy().to_string(),
                id: manifest.id.clone(),
                name: manifest.name,
                version: manifest.version,
                description: manifest.description,
                authors: manifest.authors,
                homepage: manifest.homepage,
                surface_kinds: manifest
                    .surface_kinds
                    .iter()
                    .map(|k| k.kind.clone())
                    .collect(),
                permissions: manifest.permissions,
                already_installed: already,
            });
        }
        Err(e) => {
            ui_state.add_error = Some(t_fmt("plugins.add_invalid_manifest", &e.to_string()));
        }
    }
}
