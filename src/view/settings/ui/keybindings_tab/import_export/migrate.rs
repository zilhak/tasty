//! 마이그레이션 카드 — jsx `IeMigrateCard`/`IeMigrateRow`. option 조합의 대체값을 녹화 슬롯이나
//! modifier 선택으로 받는다.
//!
//! 경계: detail 본문 중 마이그레이션 카드 한 덩어리의 그리기와, 그 행이 고치는 대체값·녹화 슬롯이다.

use tasty_host_plugin::keybinding_bundle::option_migration::ReplacementKind;
use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{
    Button, ButtonVariant, ControlSize, TagVariant, select_or_placeholder, tag, vspace,
};

use crate::adapters::ui::icons;
use crate::adapters::ui::input::shortcuts::modifier_hint::all_modifier_combos;
use crate::i18n::{t, t_fmt, t_fmt2};

use super::super::{FieldKind, LABEL_COL_WIDTH, RecordingSlot};
use super::labels::Labels;
use super::model::{MigrationRow, MigrationValue};
use super::paint::{fixed_label, glyph_at};
use super::view_model::{MigrationView, ViewModel};
use super::{
    CONFLICT_SUMMARY_FROM, GROUP_CHEVRON_GAP, MIGRATE_CARD_BORDER, MIGRATE_CARD_FILL,
    MIGRATE_FROM_W, RECORD_SLOT_MIN_W, RECORDING_FIELD,
};

/// jsx `IeMigrateCard` — 톤 틴트 카드(헤더 · 설명 · 행들).
pub(super) fn migrate_card(
    ui: &mut egui::Ui,
    th: &Theme,
    vm: &ViewModel,
    rows: &mut [MigrationRow],
    recording_field: &mut Option<RecordingSlot>,
    labels: &Labels<'_>,
) {
    let total = rows.len();
    let left = vm.unresolved;
    let done = left == 0;
    let tone = if done {
        th.accent_success().to_egui()
    } else {
        th.accent_warning().to_egui()
    };
    egui::Frame::new()
        .fill(tone.gamma_multiply(MIGRATE_CARD_FILL))
        .stroke(egui::Stroke::new(
            th.border_width.value(),
            tone.gamma_multiply(MIGRATE_CARD_BORDER),
        ))
        .corner_radius(th.corner_radius.value())
        .inner_margin(tasty_ui_widgets::margin_all(th.spacing_md))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = th.spacing_xs.value();
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = th.spacing_sm.value();
                let glyph = if done {
                    icons::CHECK
                } else {
                    icons::ALERT_TRIANGLE
                };
                glyph_at(ui, glyph, th.icon_glyph_size_md, tone);
                ui.label(
                    egui::RichText::new(if done {
                        t("settings.keybindings.ie_migrate_title_done")
                    } else {
                        t("settings.keybindings.ie_migrate_title_pending")
                    })
                    .size(th.font_size_body.value())
                    .color(tone),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let counter = if done {
                        t_fmt2(
                            "settings.keybindings.ie_migrate_count_done",
                            &total.to_string(),
                            &total.to_string(),
                        )
                    } else {
                        t_fmt2(
                            "settings.keybindings.ie_migrate_count_pending",
                            &left.to_string(),
                            &total.to_string(),
                        )
                    };
                    ui.label(
                        egui::RichText::new(counter)
                            .monospace()
                            .size(th.font_size_caption.value())
                            .color(tone),
                    );
                });
            });
            ui.scope(|ui| {
                ui.set_max_width(th.measure_lg.value());
                ui.label(
                    egui::RichText::new(if done {
                        t("settings.keybindings.ie_migrate_desc_done")
                    } else {
                        t("settings.keybindings.ie_migrate_desc_pending")
                    })
                    .size(th.font_size_term_sm.value())
                    .color(th.text_secondary()),
                );
            });
            if vm.conflicts >= CONFLICT_SUMMARY_FROM {
                conflict_summary(ui, th, vm.conflicts);
            }
            vspace(ui, th.spacing_xs);
            for (i, (row, view)) in rows.iter_mut().zip(&vm.migration).enumerate() {
                migrate_row(ui, th, i, row, view, recording_field, labels);
            }
        });
}

/// 충돌 개수 줄 — 개수(danger 강조) 먼저, 이어서 무엇이 일어나는지. 행마다의 인라인 이유는
/// 그대로 남으므로(어느 바인딩인지를 말한다) 이 줄은 목록을 되풀이하지 않는다.
fn conflict_summary(ui: &mut egui::Ui, th: &Theme, conflicts: usize) {
    let size = th.font_size_term_sm.value();
    let mut job = egui::text::LayoutJob::default();
    job.append(
        &t_fmt(
            "settings.keybindings.ie_migrate_conflicts_count",
            &conflicts.to_string(),
        ),
        0.0,
        egui::TextFormat::simple(
            egui::FontId::proportional(size),
            th.accent_danger().to_egui(),
        ),
    );
    job.append(
        t("settings.keybindings.ie_migrate_conflicts_tail"),
        0.0,
        egui::TextFormat::simple(
            egui::FontId::proportional(size),
            th.text_secondary().to_egui(),
        ),
    );
    ui.scope(|ui| {
        ui.set_max_width(th.measure_lg.value());
        ui.label(job);
    });
}

/// jsx `IeMigrateRow` — 라벨 288 · 원래 조합 120 · → · 위젯 · trailing + 부제(충돌 · fan-out).
fn migrate_row(
    ui: &mut egui::Ui,
    th: &Theme,
    index: usize,
    row: &mut MigrationRow,
    view: &MigrationView,
    recording_field: &mut Option<RecordingSlot>,
    labels: &Labels<'_>,
) {
    let recording = recording_field
        .as_ref()
        .is_some_and(|s| s.field_id == RECORDING_FIELD && s.idx == index);
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        let w = ui.available_width();
        let (sep, _) =
            ui.allocate_exact_size(egui::vec2(w, th.border_width.value()), egui::Sense::hover());
        ui.painter().hline(
            sep.x_range(),
            sep.center().y,
            egui::Stroke::new(th.border_width.value(), th.separator.to_egui()),
        );
        vspace(ui, th.spacing_sm);
        ui.horizontal(|ui| {
            ui.set_min_height(th.item_height_interactive.value());
            ui.spacing_mut().item_spacing.x = th.spacing_md.value();
            fixed_label(
                ui,
                LABEL_COL_WIDTH,
                &view.action,
                egui::FontId::proportional(th.font_size_body.value()),
                th.text_secondary().to_egui(),
            );
            fixed_label(
                ui,
                MIGRATE_FROM_W,
                &view.from,
                egui::FontId::monospace(th.font_size_term_sm.value()),
                th.text_muted().to_egui(),
            );
            glyph_at(
                ui,
                icons::CHEVRON_RIGHT,
                th.icon_glyph_size_sm,
                th.text_muted().to_egui(),
            );
            match view.kind {
                ReplacementKind::ModifierCombo => {
                    let combos: Vec<String> = all_modifier_combos()
                        .into_iter()
                        .map(|c| c.name())
                        .filter(|n| !n.contains("option"))
                        .collect();
                    let options: Vec<String> = combos.iter().map(|n| labels.combo(n)).collect();
                    let option_refs: Vec<&str> = options.iter().map(String::as_str).collect();
                    let mut idx = match &row.value {
                        MigrationValue::Set(v) => combos.iter().position(|n| n == v),
                        _ => None,
                    };
                    // 안 고른 상태는 값이 아니다 — placeholder 색 · UI 폰트로 그리고 고르면 빠진다.
                    if select_or_placeholder(
                        ui,
                        th,
                        &format!("kb_ie_modifier_{index}"),
                        &mut idx,
                        &option_refs,
                        t("settings.keybindings.ie_pick_modifier"),
                        th.field_width_md.value(),
                        true,
                    ) && let Some(name) = idx.and_then(|i| combos.get(i))
                    {
                        row.value = MigrationValue::Set(name.clone());
                    }
                }
                ReplacementKind::Combo => {
                    if record_slot(ui, th, row, view, recording, labels) {
                        *recording_field = Some(RecordingSlot {
                            field_id: RECORDING_FIELD.to_string(),
                            idx: index,
                            field_kind: FieldKind::Combo,
                        });
                    }
                }
            }
            match (&row.value, &view.conflict) {
                (_, Some(_)) => {}
                (MigrationValue::Set(_), None) => glyph_at(
                    ui,
                    icons::CHECK,
                    th.icon_glyph_size_sm,
                    th.accent_success().to_egui(),
                ),
                (MigrationValue::Unbound, None) => {
                    tag(
                        ui,
                        th,
                        t("settings.keybindings.ie_unbound_tag"),
                        TagVariant::Default,
                        false,
                    );
                }
                (MigrationValue::Unset, None) => {
                    ui.label(
                        egui::RichText::new(t("settings.keybindings.ie_not_set"))
                            .size(th.font_size_caption.value())
                            .color(th.accent_warning()),
                    );
                    // 축 modifier 는 비울 수 없다 — "비워 두기" 는 콤보 자리에만.
                    if view.kind == ReplacementKind::Combo
                        && Button::new(t("settings.keybindings.ie_leave_unbound"))
                            .variant(ButtonVariant::Ghost)
                            .size(ControlSize::Sm)
                            .show(ui, th)
                            .clicked()
                    {
                        row.value = MigrationValue::Unbound;
                        if recording {
                            *recording_field = None;
                        }
                    }
                }
            }
        });
        if let Some(conflict) = &view.conflict {
            vspace(ui, th.spacing_xs);
            ui.horizontal(|ui| {
                ui.add_space(LABEL_COL_WIDTH.value());
                ui.spacing_mut().item_spacing.x = GROUP_CHEVRON_GAP.value();
                glyph_at(
                    ui,
                    icons::ALERT_TRIANGLE,
                    th.icon_glyph_size_sm,
                    th.accent_danger().to_egui(),
                );
                ui.label(
                    egui::RichText::new(conflict)
                        .size(th.font_size_caption.value())
                        .color(th.accent_danger()),
                );
            });
        } else if let Some(fanout) = &view.fanout {
            vspace(ui, th.spacing_xs);
            ui.horizontal(|ui| {
                ui.add_space(LABEL_COL_WIDTH.value());
                ui.label(
                    egui::RichText::new(fanout)
                        .size(th.font_size_caption.value())
                        .color(th.text_muted()),
                );
            });
        }
        vspace(ui, th.spacing_sm);
    });
}

/// 녹화 슬롯 — min 140 × 24 · mono · surface-raised · 충돌이면 danger 테두리 · 빈 값은
/// text-disabled. 클릭하면 녹화를 시작한다.
fn record_slot(
    ui: &mut egui::Ui,
    th: &Theme,
    row: &MigrationRow,
    view: &MigrationView,
    recording: bool,
    labels: &Labels<'_>,
) -> bool {
    let font = egui::FontId::monospace(th.font_size_term_sm.value());
    let (text, empty) = if recording {
        (t("settings.keybindings.hint_press_key").to_string(), false)
    } else {
        match &row.value {
            MigrationValue::Set(c) => (labels.combo(c), false),
            MigrationValue::Unbound => (t("settings.keybindings.ie_unbound").to_string(), false),
            MigrationValue::Unset => (t("settings.keybindings.ie_not_set").to_string(), true),
        }
    };
    let fg = if empty {
        th.text_disabled()
    } else {
        th.text_primary()
    };
    let galley = ui.painter().layout_no_wrap(text, font, fg.to_egui());
    let pad = th.spacing_sm.value();
    let w = (galley.rect.width() + pad * 2.0).max(RECORD_SLOT_MIN_W.value());
    let (rect, resp) = ui.allocate_exact_size(
        egui::vec2(w, th.item_height_tab.value()),
        egui::Sense::click(),
    );
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    let border = if view.conflict.is_some() {
        th.accent_danger()
    } else if recording {
        th.accent_primary()
    } else {
        th.border_default()
    };
    ui.painter().rect(
        rect,
        th.corner_radius.value(),
        th.surface_raised().to_egui(),
        egui::Stroke::new(th.border_width.value(), border.to_egui()),
        egui::StrokeKind::Inside,
    );
    ui.painter().galley(
        egui::pos2(
            rect.left() + pad,
            rect.center().y - galley.rect.height() * 0.5,
        ),
        galley,
        egui::Color32::PLACEHOLDER,
    );
    resp.clicked()
}
