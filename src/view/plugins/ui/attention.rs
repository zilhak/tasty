//! 서명·권한·실행 오류로 확인이 필요한 플러그인과 가능한 조치를 표시한다.

use crate::i18n::t;
use crate::theme;
use tasty_type_geometry::length::LogicalPx;

/// semantic 역할을 지정하지 않은 primitive 폰트 크기. ui_scale을 적용하지 않는다.
/// 토큰으로 바꾸면 표시 크기가 달라질 수 있어 디자인 검토가 필요하다(ADR-0035).
const ATTN_PRIMITIVE_12: LogicalPx = LogicalPx(12.0);

/// severity 점의 기존 지름. status-dot 토큰으로 바꾸면 크기·배율 동작이 달라진다.
const ATTN_STATUS_DOT_SIZE: LogicalPx = LogicalPx(7.0);

use super::{AttentionEntry, AttentionKind, PluginsAction, PluginsSnapshot, PluginsUiState};
use tasty_ui_widgets::tokens::{PLUGIN_LIST_ROW_HEIGHT, STRUCT_GAP_2};
use tasty_ui_widgets::{
    PluginAvatarSize, hspace, margin_all, margin_sym, paint_plugin_avatar, plugin_avatar, vspace,
};

/// 사유별 (라벨 키, 설명 키). 색은 `AttentionKind::is_danger` 로 분기.
fn reason_text(kind: AttentionKind) -> (&'static str, &'static str) {
    match kind {
        AttentionKind::UnknownKey => (
            "plugins.attn_unknown_key_label",
            "plugins.attn_unknown_key_blurb",
        ),
        AttentionKind::SignatureInvalid => (
            "plugins.attn_sig_invalid_label",
            "plugins.attn_sig_invalid_blurb",
        ),
        AttentionKind::PermissionsChanged => (
            "plugins.attn_perm_changed_label",
            "plugins.attn_perm_changed_blurb",
        ),
        AttentionKind::HealthError => ("plugins.attn_health_label", "plugins.attn_health_blurb"),
    }
}

fn sev_color(th: &theme::Theme, kind: AttentionKind) -> egui::Color32 {
    if kind.is_danger() {
        egui::Color32::from(th.accent_danger())
    } else {
        egui::Color32::from(th.accent_warning())
    }
}

pub(super) fn draw_attention_tab(
    ctx: &egui::Context,
    snapshot: &PluginsSnapshot,
    ui_state: &mut PluginsUiState,
    actions: &mut Vec<PluginsAction>,
) {
    let th = theme::theme();
    let items = &snapshot.attention;

    // 선택 보정 — 비어있거나 선택이 사라졌으면 첫 항목으로.
    let valid = ui_state
        .attention_selected_id
        .as_ref()
        .is_some_and(|id| items.iter().any(|e| &e.id == id));
    if !valid {
        ui_state.attention_selected_id = items.first().map(|e| e.id.clone());
    }

    egui::SidePanel::left("plugins_attention_list")
        .exact_width(th.plugins_side_panel_width().value())
        .resizable(false)
        .show(ctx, |ui| {
            vspace(ui, th.spacing_sm);
            if items.is_empty() {
                return; // 빈 상태는 CentralPanel 에서 안내.
            }
            egui::ScrollArea::vertical()
                .drag_to_scroll(false)
                .show(ui, |ui| {
                    for entry in items {
                        let selected = ui_state.attention_selected_id.as_ref() == Some(&entry.id);
                        let color = sev_color(&th, entry.kind);
                        // 행 높이는 아바타에서 나온다 — 디자인 행이 `padding: space-sm`
                        // 위아래에 32px 아바타가 앉는 flex 행이다.
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
                            &entry.name,
                            egui::FontId::proportional(th.font_size_body.value()),
                            visuals.text_color(),
                        );
                        let (label_key, _) = reason_text(entry.kind);
                        ui.painter().text(
                            name_pos + egui::vec2(0.0, 18.0),
                            egui::Align2::LEFT_TOP,
                            t(label_key),
                            egui::FontId::proportional(th.font_size_micro.value()),
                            color,
                        );
                        // 우측 severity dot.
                        let dot_center = egui::pos2(rect.max.x - 12.0, rect.center().y);
                        ui.painter().circle_filled(
                            dot_center,
                            ATTN_STATUS_DOT_SIZE.value() * 0.5,
                            color,
                        );
                        if resp.clicked() {
                            ui_state.attention_selected_id = Some(entry.id.clone());
                        }
                        vspace(ui, STRUCT_GAP_2);
                    }
                });
        });

    egui::CentralPanel::default().show(ctx, |ui| {
        if items.is_empty() {
            draw_empty_state(ui, &th);
            return;
        }
        let selected = ui_state
            .attention_selected_id
            .as_ref()
            .and_then(|id| items.iter().find(|e| &e.id == id))
            .cloned();
        let Some(entry) = selected else {
            vspace(ui, th.spacing_xl);
            ui.label(t("plugins.none_selected"));
            return;
        };
        draw_detail(ui, &th, &entry, actions);
    });
}

/// 확인 필요 plugin 0 건 — success 톤 빈 상태.
fn draw_empty_state(ui: &mut egui::Ui, th: &theme::Theme) {
    vspace(ui, th.spacing_xl * 2.0);
    ui.vertical_centered(|ui| {
        ui.label(
            egui::RichText::new(t("plugins.attn_empty_title"))
                .size(th.font_size_heading.value())
                .color(egui::Color32::from(th.text_secondary())),
        );
        vspace(ui, th.spacing_xs);
        ui.label(
            egui::RichText::new(t("plugins.attn_empty_body"))
                .size(ATTN_PRIMITIVE_12.value())
                .color(egui::Color32::from(th.text_muted())),
        );
    });
}

fn draw_detail(
    ui: &mut egui::Ui,
    th: &theme::Theme,
    entry: &AttentionEntry,
    actions: &mut Vec<PluginsAction>,
) {
    let color = sev_color(th, entry.kind);
    let (label_key, blurb_key) = reason_text(entry.kind);

    vspace(ui, th.spacing_sm);
    egui::ScrollArea::vertical()
        .drag_to_scroll(false)
        .show(ui, |ui| {
            // identity
            // 디자인은 아바타(46) 좌, 이름줄 + 메타줄을 오른쪽 열에 쌓는다.
            ui.horizontal_top(|ui| {
                plugin_avatar(ui, th, &entry.name, PluginAvatarSize::Detail);
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.heading(&entry.name);
                        super::tag(ui, th, &format!("v{}", entry.version));
                        if entry.builtin {
                            super::tag(ui, th, t("plugins.builtin_badge"));
                        }
                    });
                    ui.label(
                        egui::RichText::new(&entry.id)
                            .small()
                            .color(egui::Color32::from(th.text_muted())),
                    );
                    if !entry.authors.is_empty() {
                        ui.label(
                            egui::RichText::new(entry.authors.join(", "))
                                .small()
                                .color(egui::Color32::from(th.text_muted())),
                        );
                    }
                });
            });
            vspace(ui, th.spacing_md);

            // 사유 배너 (severity 색 프레임) — tinted 채움/테두리 짝
            // (`tint-fill-alpha` / `tint-border-alpha`).
            egui::Frame::new()
                .fill(color.gamma_multiply(th.tint_fill_alpha()))
                .stroke(egui::Stroke::new(
                    th.border_width.value(),
                    color.gamma_multiply(th.tint_border_alpha()),
                ))
                .corner_radius(th.corner_radius.value())
                .inner_margin(margin_all(th.spacing_md))
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(t(label_key))
                            .strong()
                            .size(th.font_size_body.value())
                            .color(color),
                    );
                    vspace(ui, th.spacing_xs);
                    ui.label(
                        egui::RichText::new(t(blurb_key))
                            .size(ATTN_PRIMITIVE_12.value())
                            .color(egui::Color32::from(th.text_secondary())),
                    );
                });

            vspace(ui, th.spacing_md);
            draw_reason_detail(ui, th, entry);

            vspace(ui, th.spacing_md);
            ui.separator();
            vspace(ui, th.spacing_sm);
            draw_action_bar(ui, entry, color, actions);
        });
}

/// 사유별 추가 정보 — 권한 diff / 서명 지문 / health 상세.
fn draw_reason_detail(ui: &mut egui::Ui, th: &theme::Theme, entry: &AttentionEntry) {
    let mono_header = |ui: &mut egui::Ui, key: &str| {
        ui.label(
            egui::RichText::new(t(key))
                .size(th.font_size_micro.value())
                .color(egui::Color32::from(th.text_muted())),
        );
    };
    match entry.kind {
        AttentionKind::PermissionsChanged => {
            mono_header(ui, "plugins.attn_permission_changes");
            vspace(ui, th.spacing_xs);
            for p in &entry.permissions_added {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("+")
                            .monospace()
                            .strong()
                            .color(egui::Color32::from(th.accent_success())),
                    );
                    ui.label(
                        egui::RichText::new(p)
                            .monospace()
                            .size(ATTN_PRIMITIVE_12.value()),
                    );
                    ui.label(
                        egui::RichText::new(t("plugins.attn_newly_requested"))
                            .size(th.font_size_caption.value())
                            .color(egui::Color32::from(th.text_muted())),
                    );
                });
            }
            for p in &entry.permissions_removed {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("−")
                            .monospace()
                            .strong()
                            .color(egui::Color32::from(th.text_muted())),
                    );
                    ui.label(
                        egui::RichText::new(p)
                            .monospace()
                            .size(ATTN_PRIMITIVE_12.value())
                            .strikethrough()
                            .color(egui::Color32::from(th.text_muted())),
                    );
                    ui.label(
                        egui::RichText::new(t("plugins.attn_no_longer_used"))
                            .size(th.font_size_caption.value())
                            .color(egui::Color32::from(th.text_muted())),
                    );
                });
            }
        }
        AttentionKind::UnknownKey | AttentionKind::SignatureInvalid => {
            mono_header(ui, "plugins.attn_signature");
            vspace(ui, th.spacing_xs);
            if let Some(fp) = &entry.fingerprint {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(t("plugins.attn_fingerprint"))
                            .size(th.font_size_caption.value())
                            .color(egui::Color32::from(th.text_secondary())),
                    );
                    ui.label(
                        egui::RichText::new(fp)
                            .monospace()
                            .size(th.font_size_caption.value())
                            .color(egui::Color32::from(th.text_muted())),
                    );
                });
            }
        }
        AttentionKind::HealthError => {
            if let Some(detail) = &entry.health_detail {
                mono_header(ui, "plugins.attn_error");
                vspace(ui, th.spacing_xs);
                egui::Frame::new()
                    .fill(egui::Color32::from(th.bg_panel()))
                    .stroke(egui::Stroke::new(
                        th.border_width.value(),
                        egui::Color32::from(th.separator),
                    ))
                    .corner_radius(th.corner_radius.value())
                    .inner_margin(margin_sym(th.spacing_md, th.spacing_sm))
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new(detail)
                                .monospace()
                                .size(ATTN_PRIMITIVE_12.value())
                                .color(egui::Color32::from(th.accent_danger())),
                        );
                    });
            }
        }
    }
}

/// 상태 텍스트 + 사유별 조치 버튼.
fn draw_action_bar(
    ui: &mut egui::Ui,
    entry: &AttentionEntry,
    color: egui::Color32,
    actions: &mut Vec<PluginsAction>,
) {
    let th = crate::theme::theme();
    ui.horizontal(|ui| {
        let status_key = if entry.kind.is_danger() {
            "plugins.attn_not_registered"
        } else {
            "plugins.attn_needs_review"
        };
        ui.painter().circle_filled(
            ui.cursor().min
                + egui::vec2(
                    ATTN_STATUS_DOT_SIZE.value() * 0.5,
                    ui.text_style_height(&egui::TextStyle::Body) / 2.0,
                ),
            ATTN_STATUS_DOT_SIZE.value() * 0.5,
            color,
        );
        hspace(ui, th.spacing_md);
        ui.label(
            egui::RichText::new(t(status_key))
                .size(ATTN_PRIMITIVE_12.value())
                .color(color),
        );

        ui.with_layout(
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| match entry.kind {
                AttentionKind::PermissionsChanged => {
                    if ui.button(t("plugins.attn_reapprove")).clicked() {
                        actions.push(PluginsAction::Reapprove {
                            id: entry.id.clone(),
                        });
                    }
                }
                AttentionKind::HealthError => {
                    if ui.button(t("plugins.configure")).clicked() {
                        actions.push(PluginsAction::OpenSettings);
                    }
                }
                AttentionKind::UnknownKey | AttentionKind::SignatureInvalid => {
                    let enabled = entry.fingerprint.is_some();
                    if ui
                        .add_enabled(
                            enabled,
                            egui::Button::new(t("plugins.attn_copy_fingerprint")),
                        )
                        .clicked()
                        && let Some(fp) = &entry.fingerprint
                    {
                        ui.ctx().copy_text(fp.clone());
                    }
                }
            },
        );
    });
}
