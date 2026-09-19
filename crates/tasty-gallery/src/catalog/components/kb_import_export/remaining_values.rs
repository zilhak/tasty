//! Spec 5 — 첫 회차가 비워 둔 나머지(jsx gallery Spec "Open values — unknown failure reason,
//! counts of one, and the 620 cap").
//!
//! 경계: Spec 4 와 같은 블록 레시피를 **다른 갈래**로 든다 — 사유를 모르는 내보내기 실패.
//! 그리기는 `notices` 의 레시피를 그대로 부른다.

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::ButtonVariant;

use crate::catalog::icons;
use crate::catalog::spec::{self, StageVariant, TokenChip};

use super::notices::notice_block;
use super::paint::{caption, intro_secondary};
use super::{IE_FILE, SPECIMEN_W};

/// 사유를 모르는 실패의 가운데 구절 — 고정 집합의 catch-all. OS 가 낸 문장이 여기 들어가지
/// **않는다**는 것이 이 갈래의 결정이다.
const UNKNOWN_CLAUSE: &str = "the write didn't finish.";
/// OS 가 낸 문장 — 문장 안이 아니라 아래 제 줄에 싣는다. 한 줄, 말줄임, 전문은 tooltip.
const OS_MESSAGE: &str = "os error 28: No space left on device";

pub fn draw_remaining_values(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Tight, |ui| {
        ui.vertical(|ui| {
            ui.set_width(SPECIMEN_W.value());
            ui.spacing_mut().item_spacing.y = theme.spacing_md.value();
            caption(ui, theme, "1 · export failure — cause unknown to Tasty");
            unknown_export_failure(ui, theme);
        });
    });

    spec::meta(
        ui,
        theme,
        &[
            (
                "clause set",
                "read-only · permission · disk full · the write didn't finish.",
            ),
            (
                "OS text",
                "own line, mono 11, muted — never in the sentence",
            ),
            (
                "first / last sentence",
                "never change (as in parse failure)",
            ),
            ("one line", "ellipsised; the full text is the tooltip"),
            (
                "actions",
                "unchanged — Try again · Choose another location…",
            ),
        ],
        &[
            TokenChip::new(
                "accent-danger",
                "failure block",
                theme.accent_danger().to_egui(),
            ),
            TokenChip::new("text-muted", "OS reason line", theme.text_muted().to_egui()),
        ],
    );
    spec::note(
        ui,
        theme,
        "Why the clause is a fixed set of four. The sentence has to read the same in three \
         languages, so its middle cannot be whatever the OS happened to say. Tasty maps what it \
         can recognise — read-only, permission, disk full — and everything else takes one \
         catch-all clause that still says the one thing that matters: the write did not finish.",
    );
    spec::dont(
        ui,
        theme,
        "Don't build the OS message into the sentence (\"… — os error 28: No space left on \
         device.\"). It breaks the sentence's grammar in three languages and buries the two parts \
         the user can act on: which path, and that nothing was written.",
    );
}

/// jsx `IeExportFailG reason="other"` — 같은 블록, catch-all 구절 + OS 줄 하나.
fn unknown_export_failure(ui: &mut egui::Ui, theme: &Theme) {
    notice_block(
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
                &format!("~/tasty/{IE_FILE} — {UNKNOWN_CLAUSE} Nothing was written."),
            );
            os_reason_line(ui, theme, OS_MESSAGE);
        },
        &[
            ("Try again", ButtonVariant::Secondary),
            ("Choose another location…", ButtonVariant::Ghost),
        ],
    );
}

/// OS 가 낸 문장 한 줄 — 본문 아래 `space-xs`, mono caption, muted, 한 줄 말줄임.
pub(super) fn os_reason_line(ui: &mut egui::Ui, theme: &Theme, message: &str) {
    ui.add_space(theme.spacing_xs.value());
    ui.add(
        egui::Label::new(
            egui::RichText::new(message)
                .size(theme.font_size_caption.value())
                .family(egui::FontFamily::Monospace)
                .color(theme.text_muted().to_egui()),
        )
        .truncate(),
    )
    .on_hover_text(message);
}
