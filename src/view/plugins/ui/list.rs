use crate::i18n::t;
use crate::theme;

use super::{PluginsAction, PluginsSnapshot, PluginsUiState};
use tasty_ui_widgets::tokens::{PLUGIN_LIST_ROW_HEIGHT, STRUCT_GAP_2};
use tasty_ui_widgets::{
    PluginAvatarSize, PluginDetailBarView, PluginInstallPathsView, TagVariant, margin_sym,
    paint_plugin_avatar, plugin_avatar, plugin_command_row, plugin_detail_bar,
    plugin_detail_bar_height, plugin_detail_description, plugin_detail_meta,
    plugin_detail_name_row, plugin_detail_section, plugin_detail_section_gap, plugin_install_paths,
    tag, vspace,
};

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
            egui::ScrollArea::vertical()
                .drag_to_scroll(false)
                .show(ui, |ui| {
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

                        // 이름과 버전 두 줄을 한 클릭 영역으로 묶기 위해 직접 그린다.
                        let row_h = PLUGIN_LIST_ROW_HEIGHT.value();
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
                        let pad = egui::vec2(th.spacing_sm.value(), th.spacing_sm.value());
                        let avatar = PluginAvatarSize::Row.side().value();
                        paint_plugin_avatar(
                            ui.painter(),
                            &th,
                            egui::pos2(rect.min.x + pad.x + avatar * 0.5, rect.center().y),
                            &entry.name,
                            PluginAvatarSize::Row,
                        );
                        // 텍스트 열은 아바타 다음 — 디자인 flex 행의 `gap: space-sm`.
                        let name_pos =
                            rect.min + pad + egui::vec2(avatar + th.spacing_sm.value(), 0.0);
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

    let selected_entry = ui_state
        .selected_id
        .as_ref()
        .and_then(|id| snapshot.plugins.iter().find(|p| &p.id == id))
        .cloned();
    let confirming = selected_entry
        .as_ref()
        .is_some_and(|e| ui_state.confirm_uninstall_id.as_ref() == Some(&e.id));

    // 액션 바는 상세 열의 여백 밖, 열 폭 전체에 붙인다. 본문만 기본 패널 여백 안에 둔다.
    // 키보드 초점 순서가 화면 순서(본문 → 바)를 따르도록 같은 패널 안에서 본문을 먼저 만든다.
    let panel_frame = egui::Frame::central_panel(&ctx.style());
    let margin = panel_frame.inner_margin;
    egui::CentralPanel::default()
        .frame(panel_frame.inner_margin(egui::Margin::ZERO))
        .show(ctx, |ui| {
            let full = ui.max_rect();
            let bar_h = if selected_entry.is_some() {
                plugin_detail_bar_height(&th)
            } else {
                0.0
            };
            let split = full.max.y - bar_h;
            let body_rect = egui::Rect::from_min_max(
                full.min + egui::vec2(margin.leftf(), margin.topf()),
                egui::pos2(full.max.x - margin.rightf(), split),
            );
            let mut body = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(body_rect)
                    .layout(egui::Layout::top_down(egui::Align::Min)),
            );
            let Some(entry) = &selected_entry else {
                vspace(&mut body, th.spacing_xl);
                body.label(t("plugins.none_selected"));
                return;
            };
            vspace(&mut body, th.spacing_sm);
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .drag_to_scroll(false)
                .show(&mut body, |ui| {
                    draw_detail_body(ui, &th, entry, actions);
                    if confirming {
                        vspace(ui, th.spacing_lg);
                        draw_uninstall_confirm(ui, &th, entry, ui_state, actions);
                    }
                });

            let mut bar_ui = ui.new_child(egui::UiBuilder::new().max_rect(
                egui::Rect::from_min_max(egui::pos2(full.min.x, split), full.max),
            ));
            let bar = plugin_detail_bar(
                &mut bar_ui,
                &th,
                &PluginDetailBarView {
                    enabled: entry.enabled,
                    enabled_label: t("plugins.enabled"),
                    disabled_label: t("plugins.disabled"),
                    configure: t("plugins.configure"),
                    uninstall: t("plugins.uninstall"),
                },
            );
            if bar.toggled {
                actions.push(PluginsAction::SetEnabled {
                    id: entry.id.clone(),
                    enabled: !entry.enabled,
                });
            }
            if bar.configure {
                actions.push(PluginsAction::OpenSettings);
            }
            if bar.uninstall && !confirming {
                ui_state.confirm_uninstall_id = Some(entry.id.clone());
                ui.ctx()
                    .data_mut(|d| d.insert_temp(confirm_scroll_id(), true));
            }
        });
}

/// 제거 확인 블록이 처음 그려질 때 스크롤해 보이게 하는 일회성 표시.
fn confirm_scroll_id() -> egui::Id {
    egui::Id::new("plugins_uninstall_confirm_scroll")
}

/// 상세 본문 — identity, 설명, 오류 상자, Homepage, 절들, 설치 경로.
fn draw_detail_body(
    ui: &mut egui::Ui,
    th: &theme::Theme,
    entry: &super::PluginEntry,
    actions: &mut Vec<PluginsAction>,
) {
    // identity — 디자인은 아바타(46) 좌, 이름줄 + 메타줄을 오른쪽 열에 쌓는다.
    ui.horizontal_top(|ui| {
        plugin_avatar(ui, th, &entry.name, PluginAvatarSize::Detail);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = th.spacing_xs.value();
            let badge = entry.builtin.then(|| t("plugins.builtin_badge"));
            plugin_detail_name_row(ui, th, &entry.name, &entry.version, badge);
            // 디자인 메타 줄은 `author · cat` 이다. 매니페스트에 분류가 없어 두 번째 자리에 id 를 둔다.
            let authors = entry.authors.join(", ");
            plugin_detail_meta(ui, th, &[&authors, &entry.id]);
        });
    });
    vspace(ui, th.spacing_sm);

    if !entry.description.is_empty() {
        plugin_detail_description(ui, th, &entry.description);
        vspace(ui, th.spacing_sm);
    }

    // 디자인 error 경고 박스: spawn 반복 실패로 자동 비활성화된 plugin 에
    // 빨간 박스로 안내. config 상 enable 상태일 때만 (사용자가 끈 plugin 은
    // 정상 종료이므로 error 가 아님).
    if entry.health_error && entry.enabled {
        let danger = egui::Color32::from(th.accent_danger());
        // tinted 채움/테두리 짝 — `tint-fill-alpha` / `tint-border-alpha`.
        egui::Frame::new()
            .fill(danger.gamma_multiply(th.tint_fill_alpha()))
            .stroke(egui::Stroke::new(
                th.border_width.value(),
                danger.gamma_multiply(th.tint_border_alpha()),
            ))
            .corner_radius(th.corner_radius.value())
            .inner_margin(margin_sym(th.spacing_md, th.spacing_sm))
            .show(ui, |ui| {
                ui.label(egui::RichText::new(t("plugins.health_error")).color(danger));
            });
        vspace(ui, th.spacing_sm);
    }

    if !entry.homepage.is_empty() {
        ui.label(format!("{}: {}", t("plugins.homepage"), entry.homepage));
    }

    // 디자인 상세는 절 사이에 구분선 없이 space-lg 만 띄운다.
    plugin_detail_section_gap(ui, th);
    plugin_detail_section(ui, th, t("plugins.surface_kinds"), |ui| {
        if entry.surface_kinds.is_empty() {
            ui.label(t("plugins.none"));
        } else {
            ui.label(entry.surface_kinds.join(", "));
        }
    });

    plugin_detail_section_gap(ui, th);
    plugin_detail_section(ui, th, t("plugins.permissions"), |ui| {
        if entry.manifest_permissions.is_empty() {
            ui.label(t("plugins.none"));
        } else {
            ui.horizontal_wrapped(|ui| {
                for token in &entry.manifest_permissions {
                    tag(ui, th, token, TagVariant::Default, false);
                }
            });
        }
    });

    if !entry.commands.is_empty() {
        plugin_detail_section_gap(ui, th);
        plugin_detail_section(ui, th, t("plugins.commands"), |ui| {
            for cmd in &entry.commands {
                plugin_command_row(ui, th, t(&cmd.title_key), cmd.keybinding.as_deref());
            }
        });
    }

    plugin_detail_section_gap(ui, th);
    let log_line = format!("{}: {}", t("plugins.log_path"), entry.log_path);
    let open_folder = plugin_install_paths(
        ui,
        th,
        &PluginInstallPathsView {
            label: t("plugins.install_path"),
            open_folder: t("plugins.open_folder"),
            install_dir: &entry.install_dir,
            log_line: &log_line,
        },
    );
    if open_folder {
        actions.push(PluginsAction::OpenInstallDir {
            path: entry.install_dir.clone(),
        });
    }
}

/// 액션 바의 Uninstall 을 누른 뒤 본문 끝에 보이는 경고와 확인·취소 버튼.
/// 디자인에는 이 단계가 없어 기존 모양을 유지한다.
fn draw_uninstall_confirm(
    ui: &mut egui::Ui,
    th: &theme::Theme,
    entry: &super::PluginEntry,
    ui_state: &mut PluginsUiState,
    actions: &mut Vec<PluginsAction>,
) {
    let warn_key = if entry.builtin {
        "plugins.uninstall_builtin_warning"
    } else {
        "plugins.uninstall_warning"
    };
    let block = ui
        .vertical(|ui| {
            ui.label(
                egui::RichText::new(t(warn_key)).color(egui::Color32::from(th.accent_attention())),
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
        })
        .response;
    let scroll = ui
        .ctx()
        .data_mut(|d| d.remove_temp::<bool>(confirm_scroll_id()))
        .unwrap_or(false);
    if scroll {
        ui.scroll_to_rect(block.rect, Some(egui::Align::Max));
    }
}

#[cfg(test)]
mod tests;
