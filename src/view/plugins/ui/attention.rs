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
    PluginAttentionBarAction, PluginAttentionBarView, PluginAvatarSize, PluginFingerprintLineView,
    PluginIdentityView, PluginMetaView, margin_all, margin_sym, paint_plugin_avatar,
    plugin_attention_bar, plugin_detail_bar_height, plugin_detail_description,
    plugin_detail_identity, plugin_fingerprint_line, plugin_mono_header,
    plugin_signature_invalid_detail, vspace,
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

    let selected = ui_state
        .attention_selected_id
        .as_ref()
        .and_then(|id| items.iter().find(|e| &e.id == id))
        .cloned();
    let Some(entry) = selected else {
        egui::CentralPanel::default().show(ctx, |ui| {
            if items.is_empty() {
                draw_empty_state(ui, &th);
            } else {
                vspace(ui, th.spacing_xl);
                ui.label(t("plugins.none_selected"));
            }
        });
        return;
    };

    // Installed 와 같이 액션 바는 상세 열의 여백 밖, 열 폭 전체로 열 아래 끝에 붙는다.
    // 키보드 초점 순서가 화면 순서(본문 → 바)를 따르도록 같은 패널 안에서 본문을 먼저 만든다.
    let panel_frame = egui::Frame::central_panel(&ctx.style());
    let margin = panel_frame.inner_margin;
    egui::CentralPanel::default()
        .frame(panel_frame.inner_margin(egui::Margin::ZERO))
        .show(ctx, |ui| {
            let full = ui.max_rect();
            let split = full.max.y - plugin_detail_bar_height(&th);
            let body_rect = egui::Rect::from_min_max(
                full.min + egui::vec2(margin.leftf(), margin.topf()),
                egui::pos2(full.max.x - margin.rightf(), split),
            );
            let mut body = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(body_rect)
                    .layout(egui::Layout::top_down(egui::Align::Min)),
            );
            draw_detail(&mut body, &th, &entry);

            let mut bar_ui = ui.new_child(egui::UiBuilder::new().max_rect(
                egui::Rect::from_min_max(egui::pos2(full.min.x, split), full.max),
            ));
            draw_action_bar(&mut bar_ui, &th, &entry, actions);
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

fn draw_detail(ui: &mut egui::Ui, th: &theme::Theme, entry: &AttentionEntry) {
    let color = sev_color(th, entry.kind);
    let (label_key, blurb_key) = reason_text(entry.kind);

    vspace(ui, th.spacing_sm);
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .drag_to_scroll(false)
        .show(ui, |ui| {
            // identity — Installed 와 같은 위젯(아바타 · 이름 줄 · `작성자 · id · homepage` 메타 줄).
            let authors = entry.authors.join(", ");
            let open_homepage = plugin_detail_identity(
                ui,
                th,
                &PluginIdentityView {
                    name: &entry.name,
                    version: &entry.version,
                    builtin_tag: entry.builtin.then(|| t("plugins.builtin_badge")),
                    meta: PluginMetaView {
                        authors: &authors,
                        id: &entry.id,
                        homepage: &entry.homepage,
                    },
                },
            );
            if open_homepage && !crate::terminal_link::open_uri(&entry.homepage) {
                tracing::warn!(homepage = %entry.homepage, "plugin homepage did not open");
            }
            vspace(ui, th.spacing_md);
            if !entry.description.is_empty() {
                plugin_detail_description(ui, th, &entry.description);
                vspace(ui, th.spacing_md);
            }

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
        });
}

/// 사유별 추가 정보 — 권한 diff / 서명 지문 / health 상세.
fn draw_reason_detail(ui: &mut egui::Ui, th: &theme::Theme, entry: &AttentionEntry) {
    // 디자인 `Mono` 머리글(대문자 mono micro · text-muted · letter-spacing-caps).
    let mono_header = |ui: &mut egui::Ui, key: &str| plugin_mono_header(ui, th, t(key));
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
            // 서명 무효는 fingerprint가 가리킬 서명 자체가 깨졌으므로 줄을 두지 않고
            // 고정 설명과 실패 원인을 둔다.
            if entry.kind == AttentionKind::SignatureInvalid {
                plugin_signature_invalid_detail(
                    ui,
                    th,
                    t("plugins.attn_sig_invalid_note"),
                    entry.cause.as_deref(),
                );
            } else if let Some(fp) = &entry.fingerprint {
                fingerprint_line(ui, th, fp);
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
                        th.separator.to_egui_premultiplied(),
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

/// fingerprint 라벨, 줄인 값, 복사 IconButton을 한 줄에 그린다. 툴팁과 복사는 전체 값이다.
pub(super) fn fingerprint_line(ui: &mut egui::Ui, th: &theme::Theme, fingerprint: &str) {
    plugin_fingerprint_line(
        ui,
        th,
        &PluginFingerprintLineView {
            label: t("plugins.attn_fingerprint"),
            value: fingerprint,
            copy_tooltip: t("plugins.attn_copy_fingerprint"),
        },
    );
}

/// 상태 점과 문구, 사유별 조치 버튼 — Installed 와 같은 바 틀의 공용 위젯.
fn draw_action_bar(
    ui: &mut egui::Ui,
    th: &theme::Theme,
    entry: &AttentionEntry,
    actions: &mut Vec<PluginsAction>,
) {
    let status_key = if entry.kind.is_danger() {
        "plugins.attn_not_registered"
    } else {
        "plugins.attn_needs_review"
    };
    let action = match entry.kind {
        AttentionKind::PermissionsChanged => Some(PluginAttentionBarAction::Reapprove(t(
            "plugins.attn_reapprove",
        ))),
        AttentionKind::HealthError => {
            Some(PluginAttentionBarAction::Configure(t("plugins.configure")))
        }
        // 복사는 fingerprint 줄의 IconButton이 맡으므로 액션 바에 버튼을 두지 않는다.
        AttentionKind::UnknownKey | AttentionKind::SignatureInvalid => None,
    };
    let clicked = plugin_attention_bar(
        ui,
        th,
        &PluginAttentionBarView {
            status: t(status_key),
            color: sev_color(th, entry.kind),
            action,
        },
    );
    if clicked {
        actions.push(match entry.kind {
            AttentionKind::PermissionsChanged => PluginsAction::Reapprove {
                id: entry.id.clone(),
            },
            _ => PluginsAction::OpenSettings,
        });
    }
}

#[cfg(test)]
mod tests;
