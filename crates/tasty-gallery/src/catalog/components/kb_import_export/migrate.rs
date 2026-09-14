//! Spec 3 — Option 마이그레이션 카드(jsx `IeMigrateCard`/`IeMigrateRow`): 미완료 · 완료 · 충돌 ·
//! unbound, 그리고 같은 Spec 아래 놓이는 안내 묶음.
//!
//! 경계: 본체 `import_export/migrate.rs` 와 같은 자리 — 마이그레이션 카드 한 덩어리의 그리기다.

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{
    Button, ButtonVariant, ControlSize, TagVariant, select, select_or_placeholder, tag,
};

use crate::catalog::icons;
use crate::catalog::spec::{self, StageVariant, TokenChip};

use super::notices::notices;
use super::paint::{caption, fixed_label, glyph_at, intro_secondary};
use super::{
    CONFLICT_SUMMARY_FROM, GROUP_CHEVRON_GAP, IE_PICK, MIGRATE_CARD_BORDER, MIGRATE_CARD_FILL,
    MIGRATE_FROM_W, MIGRATE_LABEL_W, MIGRATION_H, MODIFIER_OPTIONS, MigrateRow, MigrateState,
    RECORD_SLOT_MIN_W, SPECIMEN_W, STATE, State, Widget, detail_frame,
};

// ── Spec 3: Option 마이그레이션 — 미완료 · 완료 · 충돌 · unbound · 불필요 · 실패 ─────────

pub fn draw_migration(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Tight, |ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = theme.spacing_md.value();
            STATE.with(|s| {
                let st = &mut *s.borrow_mut();
                // back bar(미해결 2) 아래 미완료 카드 — Apply 비활성.
                detail_frame(
                    ui,
                    theme,
                    "kb_ie_migration",
                    MIGRATION_H,
                    2,
                    st,
                    |ui, theme, st| {
                        migrate_card(ui, theme, false, st);
                    },
                );
            });
            caption(ui, theme, "resolved — Apply enabled");
            ui.scope(|ui| {
                ui.set_width(SPECIMEN_W.value());
                STATE.with(|s| migrate_card(ui, theme, true, &mut s.borrow_mut()));
            });
            caption(
                ui,
                theme,
                "notices — dropped plugin overrides · no migration needed · unreadable file",
            );
            ui.scope(|ui| {
                ui.set_width(SPECIMEN_W.value());
                notices(ui, theme);
            });
        });
    });

    spec::meta(
        ui,
        theme,
        &[
            ("position", "above the diff table; notice between"),
            ("gate", "Apply disabled while any row is unresolved"),
            (
                "counter",
                "“{n} of {m} unresolved” in the card header + back bar",
            ),
            ("widget A", "record slot — min 140 × 24, mono"),
            ("widget B", "modifier Select — 7 combos (non-macOS)"),
            ("label column", "288px — ja longest label measures 255px"),
            ("axis fan-out", "sub-line: “10 slots change with it”"),
            ("unbound", "counts as resolved, shown as a Tag"),
            ("not needed", "card absent + one intro sentence"),
            ("failure", "inline block in the detail area"),
        ],
        &[
            TokenChip::new(
                "accent-warning",
                "pending card + “Not set”",
                theme.accent_warning().to_egui(),
            ),
            TokenChip::new(
                "accent-success",
                "resolved card + set check",
                theme.accent_success().to_egui(),
            ),
            TokenChip::new(
                "accent-danger",
                "conflict border + parse failure",
                theme.accent_danger().to_egui(),
            ),
            TokenChip::new(
                "surface-raised",
                "record slot bed",
                theme.surface_raised().to_egui(),
            ),
            TokenChip::new(
                "text-disabled",
                "empty slot label",
                theme.text_disabled().to_egui(),
            ),
        ],
    );
    spec::note(
        ui,
        theme,
        "Two disabled Applies, two reasons. Preset shows a disabled button relabelled Applied \
         (nothing left to do). Here the label stays Apply and the reason is carried next to it \
         as “{n} unresolved” plus the card counter — a disabled button whose cause is off-screen \
         is a dead end, and relabelling would claim the import already happened.",
    );
    spec::dont(
        ui,
        theme,
        "Don't make “dropped plugin overrides” a warning callout. Nothing is wrong and there is \
         no action — a warning triangle on an unactionable fact trains people to ignore \
         triangles. It is one muted info line naming the plugins.",
    );
}

/// jsx `IeMigrateCard` — 미완료 · 완료 데모 데이터로 카드를 세운다.
fn migrate_card(ui: &mut egui::Ui, theme: &Theme, done: bool, st: &mut State) {
    let rows: &[MigrateRow] = if done {
        &[
            MigrateRow {
                action: "Screenshot to clipboard",
                from: "Option+Shift+4",
                widget: Widget::Record,
                value: "Ctrl+Shift+4",
                state: MigrateState::Set,
                conflict: None,
                fanout: None,
            },
            MigrateRow {
                action: "Category axis modifier",
                from: "Option",
                widget: Widget::Modifier,
                value: "Ctrl+Alt",
                state: MigrateState::Set,
                conflict: None,
                fanout: Some("10 slots on this axis change with it"),
            },
            MigrateRow {
                action: "Jump to error",
                from: "Option+E",
                widget: Widget::Record,
                value: "Unbound",
                state: MigrateState::Unbound,
                conflict: None,
                fanout: None,
            },
        ]
    } else {
        &[
            MigrateRow {
                action: "Screenshot to clipboard",
                from: "Option+Shift+4",
                widget: Widget::Record,
                value: "Ctrl+Shift+4",
                state: MigrateState::Set,
                conflict: None,
                fanout: None,
            },
            MigrateRow {
                action: "Category axis modifier",
                from: "Option",
                widget: Widget::Modifier,
                value: "",
                state: MigrateState::Unset,
                conflict: None,
                fanout: Some("10 slots on this axis change with it"),
            },
            MigrateRow {
                action: "Toggle vi mode",
                from: "Option+V",
                widget: Widget::Record,
                value: "Ctrl+Shift+C",
                state: MigrateState::Conflict,
                conflict: Some("Also bound to Copy"),
                fanout: None,
            },
            MigrateRow {
                action: "Jump to error",
                from: "Option+E",
                widget: Widget::Record,
                value: "Not set",
                state: MigrateState::Unset,
                conflict: None,
                fanout: None,
            },
        ]
    };
    let total = if done { 4 } else { rows.len() };
    let left = rows
        .iter()
        .filter(|r| r.state == MigrateState::Unset)
        .count();
    card(ui, theme, done, rows, (left, total), st);
}

/// jsx `IeMigrateCard` — 톤 틴트 카드(헤더 · 설명 · 충돌 개수 줄 · 행들). `counter` 는
/// (미해결, 전체).
pub(super) fn card(
    ui: &mut egui::Ui,
    theme: &Theme,
    done: bool,
    rows: &[MigrateRow],
    counter: (usize, usize),
    st: &mut State,
) {
    let (left, total) = counter;
    let tone = if done {
        theme.accent_success().to_egui()
    } else {
        theme.accent_warning().to_egui()
    };
    let conflicts = rows.iter().filter(|r| r.conflict.is_some()).count();
    egui::Frame::new()
        .fill(tone.gamma_multiply(MIGRATE_CARD_FILL))
        .stroke(egui::Stroke::new(
            theme.border_width.value(),
            tone.gamma_multiply(MIGRATE_CARD_BORDER),
        ))
        .corner_radius(theme.corner_radius.value())
        .inner_margin(tasty_ui_widgets::margin_all(theme.spacing_md))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = theme.spacing_xs.value();
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
                let glyph = if done {
                    icons::CHECK
                } else {
                    icons::ALERT_TRIANGLE
                };
                glyph_at(ui, glyph, theme.icon_glyph_size_md, tone);
                ui.label(
                    egui::RichText::new(if done {
                        "Option bindings resolved"
                    } else {
                        "Option bindings need a replacement"
                    })
                    .size(theme.font_size_body.value())
                    .color(tone),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let counter = if done {
                        format!("{total} of {total} resolved")
                    } else {
                        format!("{left} of {total} unresolved")
                    };
                    ui.label(
                        egui::RichText::new(counter)
                            .monospace()
                            .size(theme.font_size_caption.value())
                            .color(tone),
                    );
                });
            });
            intro_secondary(
                ui,
                theme,
                if done {
                    "Every option-bearing binding now has a replacement or is left unbound. Apply \
                     is enabled."
                } else {
                    "option never matches on this OS — these bindings would look bound and do \
                     nothing. Give each one a replacement, or leave it unbound. Apply stays \
                     disabled until none are left."
                },
            );
            // 충돌 개수 줄 — 2 건부터. 행마다의 인라인 이유는 그대로 남는다.
            if conflicts >= CONFLICT_SUMMARY_FROM {
                conflict_summary(ui, theme, conflicts);
            }
            tasty_ui_widgets::vspace(ui, theme.spacing_xs);
            for (i, r) in rows.iter().enumerate() {
                migrate_row(ui, theme, r, done, i, st);
            }
        });
}

/// 충돌 개수 줄 — jsx `IeMigrateCard` 의 `conflicts > 1` 문단. 개수(danger 강조) 먼저.
fn conflict_summary(ui: &mut egui::Ui, theme: &Theme, conflicts: usize) {
    let size = theme.font_size_term_sm.value();
    let mut job = egui::text::LayoutJob::default();
    job.append(
        &format!("{conflicts} conflicts"),
        0.0,
        egui::TextFormat::simple(
            egui::FontId::proportional(size),
            theme.accent_danger().to_egui(),
        ),
    );
    job.append(
        " — those shortcuts are already bound. The shortcut-conflict popup opens on Apply.",
        0.0,
        egui::TextFormat::simple(
            egui::FontId::proportional(size),
            theme.text_secondary().to_egui(),
        ),
    );
    ui.scope(|ui| {
        ui.set_max_width(theme.measure_lg.value());
        ui.label(job);
    });
}

/// jsx `IeMigrateRow` — 라벨 288 · 원래 조합 120 · → · 위젯 · trailing(check / Not set +
/// Leave unbound / Unbound Tag) + 부제(충돌 사유 또는 fan-out).
fn migrate_row(
    ui: &mut egui::Ui,
    theme: &Theme,
    r: &MigrateRow,
    done: bool,
    index: usize,
    st: &mut State,
) {
    // jsx `padding: space-sm 0 · borderTop separator · gap 4` — 간격을 명시로만 준다.
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        // 행 상단 separator.
        let w = ui.available_width();
        let (sep, _) = ui.allocate_exact_size(
            egui::vec2(w, theme.border_width.value()),
            egui::Sense::hover(),
        );
        ui.painter().hline(
            sep.x_range(),
            sep.center().y,
            egui::Stroke::new(theme.border_width.value(), theme.separator.to_egui()),
        );
        tasty_ui_widgets::vspace(ui, theme.spacing_sm);
        ui.horizontal(|ui| {
            ui.set_min_height(theme.item_height_interactive.value());
            ui.spacing_mut().item_spacing.x = theme.spacing_md.value();
            fixed_label(
                ui,
                MIGRATE_LABEL_W,
                r.action,
                egui::FontId::proportional(theme.font_size_body.value()),
                theme.text_secondary().to_egui(),
            );
            fixed_label(
                ui,
                MIGRATE_FROM_W,
                r.from,
                egui::FontId::monospace(theme.font_size_term_sm.value()),
                theme.text_muted().to_egui(),
            );
            glyph_at(
                ui,
                icons::CHEVRON_RIGHT,
                theme.icon_glyph_size_sm,
                theme.text_muted().to_egui(),
            );
            match r.widget {
                Widget::Modifier => {
                    if done {
                        let mut idx = MODIFIER_OPTIONS
                            .iter()
                            .position(|o| *o == r.value)
                            .unwrap_or(0);
                        select(
                            ui,
                            theme,
                            &format!("kb_ie_modifier_done_{index}"),
                            &mut idx,
                            MODIFIER_OPTIONS,
                            theme.field_width_md.value(),
                            true,
                        );
                    } else {
                        select_or_placeholder(
                            ui,
                            theme,
                            &format!("kb_ie_modifier_{index}"),
                            &mut st.pending_modifier,
                            MODIFIER_OPTIONS,
                            IE_PICK,
                            theme.field_width_md.value(),
                            true,
                        );
                    }
                }
                Widget::Record => record_slot(ui, theme, r),
            }
            match r.state {
                MigrateState::Set => glyph_at(
                    ui,
                    icons::CHECK,
                    theme.icon_glyph_size_sm,
                    theme.accent_success().to_egui(),
                ),
                MigrateState::Unbound => {
                    tag(
                        ui,
                        theme,
                        "Unbound — counts as resolved",
                        TagVariant::Default,
                        false,
                    );
                }
                MigrateState::Unset => {
                    ui.label(
                        egui::RichText::new("Not set")
                            .size(theme.font_size_caption.value())
                            .color(theme.accent_warning().to_egui()),
                    );
                    // 축 modifier 는 비울 수 없다 — "Leave unbound" 는 콤보 자리에만.
                    if matches!(r.widget, Widget::Record) {
                        Button::new("Leave unbound")
                            .variant(ButtonVariant::Ghost)
                            .size(ControlSize::Sm)
                            .show(ui, theme);
                    }
                }
                MigrateState::Conflict => {}
            }
        });
        if let Some(conflict) = r.conflict {
            tasty_ui_widgets::vspace(ui, theme.spacing_xs);
            ui.horizontal(|ui| {
                ui.add_space(MIGRATE_LABEL_W.value());
                ui.spacing_mut().item_spacing.x = GROUP_CHEVRON_GAP.value();
                glyph_at(
                    ui,
                    icons::ALERT_TRIANGLE,
                    theme.icon_glyph_size_sm,
                    theme.accent_danger().to_egui(),
                );
                ui.label(
                    egui::RichText::new(format!(
                        "{conflict} — the shortcut-conflict popup opens on Apply."
                    ))
                    .size(theme.font_size_caption.value())
                    .color(theme.accent_danger().to_egui()),
                );
            });
        } else if let Some(fanout) = r.fanout {
            tasty_ui_widgets::vspace(ui, theme.spacing_xs);
            ui.horizontal(|ui| {
                ui.add_space(MIGRATE_LABEL_W.value());
                ui.label(
                    egui::RichText::new(fanout)
                        .size(theme.font_size_caption.value())
                        .color(theme.text_muted().to_egui()),
                );
            });
        }
        tasty_ui_widgets::vspace(ui, theme.spacing_sm);
    });
}

/// 녹화 슬롯 — min 140 × 24 · mono · surface-raised · 충돌이면 danger 테두리 · 빈 값은
/// text-disabled.
fn record_slot(ui: &mut egui::Ui, theme: &Theme, r: &MigrateRow) {
    let font = egui::FontId::monospace(theme.font_size_term_sm.value());
    let empty = r.state == MigrateState::Unset;
    let fg = if empty {
        theme.text_disabled()
    } else {
        theme.text_primary()
    };
    let galley = ui
        .painter()
        .layout_no_wrap(r.value.to_string(), font, fg.to_egui());
    let pad = theme.spacing_sm.value();
    let w = (galley.rect.width() + pad * 2.0).max(RECORD_SLOT_MIN_W.value());
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(w, theme.item_height_tab.value()),
        egui::Sense::click(),
    );
    let border = if r.state == MigrateState::Conflict {
        theme.accent_danger()
    } else {
        theme.border_default()
    };
    ui.painter().rect(
        rect,
        theme.corner_radius.value(),
        theme.surface_raised().to_egui(),
        egui::Stroke::new(theme.border_width.value(), border.to_egui()),
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
}
