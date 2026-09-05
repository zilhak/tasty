//! `Attention`("확인 필요") 탭 — 등록 거부(서명/신뢰) 또는 실행 실패(health
//! error) plugin 을 사유·조치와 함께 보여준다. 좌측 목록 + 우측 상세(사유 배너 +
//! 사유별 detail + 액션 바). 데이터는 `PluginsSnapshot::attention`.

use crate::i18n::t;
use crate::theme;
use tasty_type_geometry::length::LogicalPx;

// ── 디자인 스케일 밖 폰트 크기 ──────────────────────────────────────────────
//
// `Theme` 의 UI 폰트 스케일은 micro 10 · caption 11 · body/heading 13 · max 14 이고,
// DTCG primitive 에는 12 · 16 · 17 · 20 이 더 있다. 아래 값들은 **어느 tier 에도
// 없다** — 코드에서 자란 값이라 토큰으로 스냅하면 실제로 픽셀이 바뀐다. 조용히
// 반올림하지 않고 이름만 붙여 한곳에 모아 둔다(스냅 여부는 디자인 판단 항목).
//
// **`.5` 로 끝나는 값은 애초에 토큰이 될 수 없다.** 토큰 폰트 크기는
// `Theme::with_colors_and_zoom` 의 `zoomed()` 를 거치는데 그게 `.round()` 하므로
// 어떤 `ui_scale` 에서도 정수다 — "zoom 1 에서만 0.5 다" 가 아니라 전 배율에서
// 다르다. 규칙 전문은 `docs/design/systems/theme.md` "스케일 밖 폰트 값".
//
// 토큰이 아니므로 `ui_scale` 줌을 타지 않는다 — 이것도 현행 유지다. 그 대가와
// 재검토 조건(디자인이 `.5` 스케일을 정식 tier 로 승인하면 발동)은
// `docs/adr/0126-off-scale-font-values-are-not-snapped-to-tokens.md` 에 있다 —
// 위 문단은 원인이고, 근거·대안·철회 조건은 그 ADR 이 든다.

/// 알림 비어있음 상태의 제목. 스케일 밖(13.5).
const ATTN_EMPTY_TITLE_SIZE: LogicalPx = LogicalPx(13.5);
/// 사유 카드 본문. 스케일 밖(12.5).
const ATTN_REASON_BLURB_SIZE: LogicalPx = LogicalPx(12.5);
/// 서명 지문 라벨/값. 스케일 밖(11.5).
const ATTN_FINGERPRINT_SIZE: LogicalPx = LogicalPx(11.5);
/// 항목 행의 사유 라벨. 스케일 밖(10.5) — `font_size_micro`(10)와 0.5 차이라
/// 스냅하고 싶어지는 자리지만, 그 0.5 는 어떤 zoom 에서도 사라지지 않는다.
const ATTN_REASON_LABEL_SIZE: LogicalPx = LogicalPx(10.5);

/// DTCG primitive `font-size-12` 를 직접 쓰는 자리. 12px 는 primitive 에는 있지만
/// **semantic role 이 배정돼 있지 않아** `Theme` 필드가 없다 — 어느 semantic 에
/// 묶을지가 판단 항목이라 primitive 값을 그대로 이름 붙여 둔다.
const ATTN_PRIMITIVE_12: LogicalPx = LogicalPx(12.0);

/// severity 점의 지름. 스케일 밖(7) — 점 치수 토큰은 `status-dot-size`(8) 하나뿐이고,
/// 그 토큰은 `zoomed()` 를 타 배율 0.85 / 1.0 / 1.2 에서 7 / 8 / 10 이 된다. 여기를
/// 8 로 보내면 배율 1 에서 픽셀이 바뀐다 — 스냅이 아니라 값 변경이라
/// `docs/adr/0126-off-scale-font-values-are-not-snapped-to-tokens.md` 대로 이름만 붙인다.
/// **같은 7 을 `crates/tasty-ui-widgets/src/status_bar.rs` 의 `DOT_SIZE` 도 쓴다** —
/// 무관한 두 크레이트가 독립적으로 고른 값이라 드리프트가 아니라 역할일 가능성이 높고,
/// 그 판단이 서면 둘이 한 토큰으로 모인다.
const ATTN_STATUS_DOT_SIZE: LogicalPx = LogicalPx(7.0);

use super::{AttentionEntry, AttentionKind, PluginsAction, PluginsSnapshot, PluginsUiState};
use tasty_ui_widgets::tokens::STRUCT_GAP_2;
use tasty_ui_widgets::{hspace, margin_all, margin_sym, vspace};

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
            egui::ScrollArea::vertical().show(ui, |ui| {
                for entry in items {
                    let selected = ui_state.attention_selected_id.as_ref() == Some(&entry.id);
                    let color = sev_color(&th, entry.kind);
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
                        &entry.name,
                        egui::FontId::proportional(th.font_size_body.value()),
                        visuals.text_color(),
                    );
                    let (label_key, _) = reason_text(entry.kind);
                    ui.painter().text(
                        name_pos + egui::vec2(0.0, 18.0),
                        egui::Align2::LEFT_TOP,
                        t(label_key),
                        egui::FontId::proportional(ATTN_REASON_LABEL_SIZE.value()),
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
    // 48 은 그리드 스텝 밖의 값 — 새 스텝을 추가하는 대신 기존 spacing_xl(24)을
    // 2배 연산으로 표현.
    vspace(ui, th.spacing_xl * 2.0);
    ui.vertical_centered(|ui| {
        ui.label(
            egui::RichText::new(t("plugins.attn_empty_title"))
                .size(ATTN_EMPTY_TITLE_SIZE.value())
                .color(egui::Color32::from(th.text_secondary())),
        );
        // 6→4 스냅 (그리드 정합 — 레이블-내용 tight 간격).
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
    egui::ScrollArea::vertical().show(ui, |ui| {
        // identity
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
        vspace(ui, th.spacing_md);

        // 사유 배너 (severity 색 프레임).
        egui::Frame::new()
            .fill(color.gamma_multiply(0.11))
            .stroke(egui::Stroke::new(
                th.border_width.value(),
                color.gamma_multiply(0.36),
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
                        .size(ATTN_REASON_BLURB_SIZE.value())
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
            // 6→4 스냅 (그리드 정합 — 레이블-내용 tight 간격).
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
            // 6→4 스냅 (그리드 정합 — 레이블-내용 tight 간격).
            vspace(ui, th.spacing_xs);
            if let Some(fp) = &entry.fingerprint {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(t("plugins.attn_fingerprint"))
                            .size(ATTN_FINGERPRINT_SIZE.value())
                            .color(egui::Color32::from(th.text_secondary())),
                    );
                    ui.label(
                        egui::RichText::new(fp)
                            .monospace()
                            .size(ATTN_FINGERPRINT_SIZE.value())
                            .color(egui::Color32::from(th.text_muted())),
                    );
                });
            }
        }
        AttentionKind::HealthError => {
            if let Some(detail) = &entry.health_detail {
                mono_header(ui, "plugins.attn_error");
                // 6→4 스냅 (그리드 정합 — 레이블-내용 tight 간격).
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
        // 디자인 값 11px 은 off-grid — 4px 그리드의 가장 가까운 값인 spacing_md(12)로 snap.
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
