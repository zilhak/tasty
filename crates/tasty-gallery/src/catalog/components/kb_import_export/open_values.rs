//! Spec 4 — 첫 시안이 비워 둔 값 여섯(jsx gallery `IeExportFailG` · `IeBundleNoticesG` ·
//! `IeParseFailG` · `IeConflictSummaryG` · `IeModifierSelectG`).
//!
//! 경계: 본체 `import_export/` 의 `entry`(Export 행의 notice 자리) · `notices`(알림 블록
//! 레시피) · `migrate`(충돌 개수 줄 · modifier placeholder)가 그리는 상태를 한 Spec 에 모은다.
//! 그리기 자체는 그 모듈들의 것을 부른다 — 여기서 새로 짓지 않는다.

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{ButtonVariant, select, select_or_placeholder};

use crate::catalog::icons;
use crate::catalog::spec::{self, StageVariant, TokenChip};

use super::entry::action_row;
use super::migrate::card;
use super::notices::{dropped_notice, notice_block, notice_line, parse_failure};
use super::paint::{caption, fixed_label, glyph_at, intro_secondary};
use super::{
    IE_FILE, IE_PICK, MIGRATE_FROM_W, MODIFIER_OPTIONS, MigrateRow, MigrateState, NOTICE_FOLD_AT,
    SPECIMEN_W, STATE, State, Widget,
};

/// 경고 블록 데모 줄 — 고정 순서(스키마 → 모르는 액션 → 빈 그룹). 넷째 줄은 접힌다.
const NOTICE_LINES: &[&str] = &[
    "Written by a newer schema (v3, this build reads v2) — unreadable parts were skipped.",
    "2 unknown actions were skipped (pane.zoom_cycle, tab.pin).",
    "The group [keybindings.image] is empty — nothing to import from it.",
    "The group [keybindings.markdown] is empty — nothing to import from it.",
];

/// 충돌이 여럿인 카드의 데모 행 — jsx `IeConflictSummaryG`.
const CONFLICT_ROWS: &[MigrateRow] = &[
    conflict_row(
        "Toggle vi mode",
        "Option+V",
        "Ctrl+Shift+C",
        "Also bound to Copy",
    ),
    conflict_row(
        "Jump to error",
        "Option+E",
        "Ctrl+Shift+K",
        "Also bound to Clear scrollback",
    ),
    conflict_row(
        "Screenshot to clipboard",
        "Option+Shift+4",
        "Ctrl+Shift+P",
        "Also bound to Command palette",
    ),
];

const fn conflict_row(
    action: &'static str,
    from: &'static str,
    value: &'static str,
    conflict: &'static str,
) -> MigrateRow {
    MigrateRow {
        action,
        from,
        widget: Widget::Record,
        value,
        state: MigrateState::Conflict,
        conflict: Some(conflict),
        fanout: None,
    }
}

pub fn draw_open_values(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Tight, |ui| {
        ui.vertical(|ui| {
            ui.set_width(SPECIMEN_W.value());
            ui.spacing_mut().item_spacing.y = theme.spacing_md.value();
            STATE.with(|s| {
                let st = &mut *s.borrow_mut();
                caption(ui, theme, "1 · export failure — inline in the Export row");
                export_failure_row(ui, theme, st);
                caption(
                    ui,
                    theme,
                    "2 · bundle warnings — one block, info line apart",
                );
                bundle_notices(ui, theme, st);
                caption(
                    ui,
                    theme,
                    "3 · parse failure — with and without a line number",
                );
                parse_failure(ui, theme, true);
                parse_failure(ui, theme, false);
                caption(ui, theme, "4 · several conflicts — count first, from 2 up");
                card(ui, theme, false, CONFLICT_ROWS, (3, 4), st);
                caption(ui, theme, "5 · modifier Select — placeholder / chosen");
                modifier_selects(ui, theme, st);
            });
        });
    });

    spec::meta(
        ui,
        theme,
        &[
            ("export success", "toast with the resolved path"),
            ("export failure", "inline danger block in the Export row"),
            ("failure actions", "Try again · Choose another location…"),
            ("info line", "dropped overrides — muted, unchanged"),
            (
                "warning block",
                "one block, 1 line per notice, count in header",
            ),
            ("notice order", "schema → unknown actions → empty groups"),
            ("fold", "3 lines, then “Show {n} more”"),
            (
                "parse copy",
                "line clause replaced by “the file isn't TOML.”",
            ),
            ("conflicts", "count-first line from 2 up; row lines always"),
            ("placeholder", "“Select a modifier” · text-placeholder"),
        ],
        &[
            TokenChip::new(
                "accent-warning",
                "warning block + Not set",
                theme.accent_warning().to_egui(),
            ),
            TokenChip::new(
                "accent-danger",
                "failure blocks + conflict count",
                theme.accent_danger().to_egui(),
            ),
            TokenChip::new(
                "text-placeholder",
                "modifier Select before a choice",
                theme.text_placeholder().to_egui(),
            ),
        ],
    );
    spec::note(
        ui,
        theme,
        "Why the notices are one block and the info line is not in it. Tone is the sorting key, \
         not topic: a reader scans for \"is anything wrong\". Merging an unactionable fact into a \
         warning-toned block makes the whole block unactionable-looking; splitting the warnings \
         into one block each makes a wall where one thing is needed — read it, then go look at \
         the table.",
    );
    spec::dont(
        ui,
        theme,
        "Don't toast an export failure. A toast auto-dismisses and carries no retry, so the one \
         case where the user must act is the one case that disappears on its own.",
    );
}

/// jsx `IeExportFailG` — Export 행(버튼 꺼짐) + 그 안의 실패 블록. 두 액션 모두 블록을 닫고
/// 버튼을 되살린다(체크박스 없이 눌러 볼 수 있게). 닫힌 뒤에는 캡션 옆 버튼이 다시 띄운다.
fn export_failure_row(ui: &mut egui::Ui, theme: &Theme, st: &mut State) {
    let failed = st.export_failed;
    let mut dismissed = false;
    let mut notice = |ui: &mut egui::Ui| {
        let clicked = notice_block(
            ui,
            theme,
            theme.accent_danger().to_egui(),
            icons::ALERT_CIRCLE,
            "The export wasn't written",
            None,
            |ui| {
                intro_secondary(
                    ui,
                    theme,
                    &format!("~/tasty/{IE_FILE} — the folder is read-only. Nothing was written."),
                );
            },
            &[
                ("Try again", ButtonVariant::Secondary),
                ("Choose another location…", ButtonVariant::Ghost),
            ],
        );
        dismissed = clicked.is_some();
    };
    action_row(
        ui,
        theme,
        icons::DOWNLOAD,
        "Export",
        "Writes every binding — general, quick switch, script bindings and plugin overrides — \
         to one file.",
        "Export…",
        ButtonVariant::Secondary,
        !failed,
        failed.then_some(&mut notice as &mut dyn FnMut(&mut egui::Ui)),
    );
    if dismissed {
        st.export_failed = false;
    }
    if !st.export_failed
        && tasty_ui_widgets::Button::new("Show the failure again")
            .variant(ButtonVariant::Ghost)
            .size(tasty_ui_widgets::ControlSize::Sm)
            .show(ui, theme)
            .clicked()
    {
        st.export_failed = true;
    }
}

/// jsx `IeBundleNoticesG` — 정보 줄(그대로) + 경고 블록 하나(개수 헤더 · 세 줄 · 접기).
fn bundle_notices(ui: &mut egui::Ui, theme: &Theme, st: &mut State) {
    dropped_notice(ui, theme);
    let hidden = if st.notices_expanded {
        0
    } else {
        NOTICE_LINES.len().saturating_sub(NOTICE_FOLD_AT)
    };
    let shown = NOTICE_LINES.len() - hidden;
    let more = format!("Show {hidden} more");
    let actions: &[(&str, ButtonVariant)] = if hidden > 0 {
        &[(&more, ButtonVariant::Ghost)]
    } else {
        &[]
    };
    let clicked = notice_block(
        ui,
        theme,
        theme.accent_warning().to_egui(),
        icons::ALERT_TRIANGLE,
        "Read with warnings",
        Some(&format!("{} notices", NOTICE_LINES.len())),
        |ui| {
            for line in &NOTICE_LINES[..shown] {
                notice_line(ui, theme, line);
            }
        },
        actions,
    );
    if clicked.is_some() {
        st.notices_expanded = true;
    }
}

/// jsx `IeModifierSelectG` — 원래 조합 · → · Select · trailing. 안 고른 상태는 placeholder,
/// 고른 상태는 값 + check.
fn modifier_selects(ui: &mut egui::Ui, theme: &Theme, st: &mut State) {
    for chosen in [false, true] {
        ui.horizontal(|ui| {
            ui.set_min_height(theme.item_height_interactive.value());
            ui.spacing_mut().item_spacing.x = theme.spacing_md.value();
            fixed_label(
                ui,
                MIGRATE_FROM_W,
                "Option",
                egui::FontId::monospace(theme.font_size_term_sm.value()),
                theme.text_muted().to_egui(),
            );
            glyph_at(
                ui,
                icons::CHEVRON_RIGHT,
                theme.icon_glyph_size_sm,
                theme.text_muted().to_egui(),
            );
            if chosen {
                let mut idx = MODIFIER_OPTIONS
                    .iter()
                    .position(|o| *o == "Ctrl+Alt")
                    .unwrap_or(0);
                select(
                    ui,
                    theme,
                    "kb_ie_open_values_chosen",
                    &mut idx,
                    MODIFIER_OPTIONS,
                    theme.field_width_md.value(),
                    true,
                );
                glyph_at(
                    ui,
                    icons::CHECK,
                    theme.icon_glyph_size_sm,
                    theme.accent_success().to_egui(),
                );
            } else {
                select_or_placeholder(
                    ui,
                    theme,
                    "kb_ie_open_values_placeholder",
                    &mut st.pending_modifier,
                    MODIFIER_OPTIONS,
                    IE_PICK,
                    theme.field_width_md.value(),
                    true,
                );
                ui.label(
                    egui::RichText::new("Not set")
                        .size(theme.font_size_caption.value())
                        .color(theme.accent_warning().to_egui()),
                );
            }
        });
    }
}
