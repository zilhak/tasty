use crate::i18n::t;
use crate::theme;

use super::{PluginsAction, PluginsSnapshot, PluginsUiState};
use tasty_ui_widgets::tokens::STRUCT_GAP_2;
use tasty_ui_widgets::{margin_sym, vspace};

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

    // name / authors / description 부분일치 필터 (대소문자 무시).
    let needle = ui_state.filter.trim().to_lowercase();
    let visible: Vec<_> = snapshot
        .plugins
        .iter()
        .filter(|e| {
            if needle.is_empty() {
                return true;
            }
            let hay =
                format!("{} {} {}", e.name, e.authors.join(" "), e.description).to_lowercase();
            hay.contains(&needle)
        })
        .collect();

    egui::SidePanel::left("plugins_list")
        .exact_width(th.plugins_side_panel_width().value())
        .resizable(false)
        .show(ctx, |ui| {
            vspace(ui, th.spacing_sm);
            if snapshot.plugins.is_empty() {
                vspace(ui, th.spacing_lg);
                ui.label(
                    egui::RichText::new(t("plugins.empty"))
                        .color(egui::Color32::from(th.text_muted())),
                );
                return;
            }
            if visible.is_empty() {
                vspace(ui, th.spacing_lg);
                ui.label(
                    egui::RichText::new(t("plugins.no_matches"))
                        .color(egui::Color32::from(th.text_muted())),
                );
                return;
            }
            egui::ScrollArea::vertical().show(ui, |ui| {
                for entry in &visible {
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
                        egui::FontId::proportional(th.font_size_body.value()),
                        visuals.text_color(),
                    );
                    let sub_pos = name_pos + egui::vec2(0.0, 18.0);
                    ui.painter().text(
                        sub_pos,
                        egui::Align2::LEFT_TOP,
                        &sub,
                        egui::FontId::proportional(th.font_size_micro.value()),
                        egui::Color32::from(th.text_muted()),
                    );
                    // 디자인 StatusDot(danger): spawn 반복 실패로 자동 비활성화된
                    // plugin 은 행 우측에 빨간 dot 을 그린다. 상세 경고 박스와
                    // 동일하게 enable 상태인 error plugin 에만 표시한다 (사용자가
                    // 끈 plugin 은 정상 종료이므로 error 아님).
                    if entry.health_error && entry.enabled {
                        let dot_center = egui::pos2(rect.max.x - 12.0, rect.center().y);
                        tasty_ui_widgets::paint_badge_dot(
                            ui.painter(),
                            &th,
                            dot_center,
                            tasty_ui_widgets::BadgeVariant::Danger,
                        );
                    }
                    if resp.clicked() {
                        ui_state.selected_id = Some(entry.id.clone());
                    }
                    vspace(ui, STRUCT_GAP_2);
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
            vspace(ui, th.spacing_xl);
            ui.label(t("plugins.none_selected"));
            return;
        };

        vspace(ui, th.spacing_sm);
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading(&entry.name);
                super::tag(ui, &th, &format!("v{}", entry.version));
                if entry.builtin {
                    ui.label(
                        egui::RichText::new(t("plugins.builtin_badge"))
                            .small()
                            .color(egui::Color32::from(th.accent_agent())),
                    );
                }
            });
            ui.label(
                egui::RichText::new(&entry.id)
                    .small()
                    .color(egui::Color32::from(th.text_muted())),
            );
            vspace(ui, th.spacing_sm);

            if !entry.description.is_empty() {
                ui.label(&entry.description);
                vspace(ui, th.spacing_sm);
            }

            // 디자인 error 경고 박스: spawn 반복 실패로 자동 비활성화된 plugin 에
            // 빨간 박스로 안내. config 상 enable 상태일 때만 (사용자가 끈 plugin 은
            // 정상 종료이므로 error 가 아님).
            if entry.health_error && entry.enabled {
                let danger = egui::Color32::from(th.accent_danger());
                // error 박스의 채움/테두리 짝. 대응 토큰 없음.
                const ERROR_BOX_FILL_OPACITY: f32 = 0.12;
                const ERROR_BOX_STROKE_OPACITY: f32 = 0.35;
                egui::Frame::new()
                    .fill(danger.gamma_multiply(ERROR_BOX_FILL_OPACITY))
                    .stroke(egui::Stroke::new(
                        th.border_width.value(),
                        danger.gamma_multiply(ERROR_BOX_STROKE_OPACITY),
                    ))
                    .corner_radius(th.corner_radius.value())
                    .inner_margin(margin_sym(th.spacing_md, th.spacing_sm))
                    .show(ui, |ui| {
                        ui.label(egui::RichText::new(t("plugins.health_error")).color(danger));
                    });
                vspace(ui, th.spacing_sm);
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

            vspace(ui, th.spacing_md);
            ui.separator();

            vspace(ui, th.spacing_md);
            ui.horizontal(|ui| {
                ui.label(format!("{}:", t("plugins.status")));
                let mut enabled = entry.enabled;
                if ui.checkbox(&mut enabled, t("plugins.enabled")).changed() {
                    actions.push(PluginsAction::SetEnabled {
                        id: entry.id.clone(),
                        enabled,
                    });
                }
                // lifecycle 창 → per-plugin config (Settings›Plugins) 연결 고리.
                if ui.button(t("plugins.configure")).clicked() {
                    actions.push(PluginsAction::OpenSettings);
                }
            });

            vspace(ui, th.spacing_sm);
            ui.label(format!("{}:", t("plugins.surface_kinds")));
            if entry.surface_kinds.is_empty() {
                ui.label(t("plugins.none"));
            } else {
                ui.label(entry.surface_kinds.join(", "));
            }

            vspace(ui, th.spacing_md);
            ui.separator();
            vspace(ui, th.spacing_md);
            ui.label(format!("{}:", t("plugins.permissions")));
            if entry.manifest_permissions.is_empty() {
                ui.label(t("plugins.none"));
            } else {
                ui.horizontal_wrapped(|ui| {
                    for token in &entry.manifest_permissions {
                        super::tag(ui, &th, token);
                    }
                });
            }

            if !entry.commands.is_empty() {
                vspace(ui, th.spacing_md);
                ui.separator();
                vspace(ui, th.spacing_md);
                ui.label(format!("{}:", t("plugins.commands")));
                for cmd in &entry.commands {
                    ui.horizontal(|ui| {
                        ui.label(t(&cmd.title_key));
                        if let Some(kb) = &cmd.keybinding {
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    super::tag(ui, &th, kb);
                                },
                            );
                        }
                    });
                }
            }

            vspace(ui, th.spacing_md);
            ui.separator();
            vspace(ui, th.spacing_md);
            ui.label(format!("{}:", t("plugins.install_path")));
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(&entry.install_dir)
                        .small()
                        .color(egui::Color32::from(th.text_muted())),
                );
                if ui.small_button(t("plugins.open_folder")).clicked() {
                    actions.push(PluginsAction::OpenInstallDir {
                        path: entry.install_dir.clone(),
                    });
                }
            });

            // 6→4 스냅 (그리드 정합 — 메타 라벨 tight 간격).
            vspace(ui, th.spacing_xs);
            ui.label(
                egui::RichText::new(format!("{}: {}", t("plugins.log_path"), entry.log_path))
                    .small()
                    .color(egui::Color32::from(th.text_muted())),
            );

            vspace(ui, th.spacing_lg);
            if ui_state.confirm_uninstall_id.as_ref() == Some(&entry.id) {
                let warn_key = if entry.builtin {
                    "plugins.uninstall_builtin_warning"
                } else {
                    "plugins.uninstall_warning"
                };
                ui.label(
                    egui::RichText::new(t(warn_key))
                        .color(egui::Color32::from(th.accent_attention())),
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
