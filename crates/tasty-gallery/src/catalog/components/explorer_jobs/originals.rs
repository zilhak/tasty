//! 받지 않은 요청 카드와 남은 원본 다시 지우기 예제. 디자인 batch 11 의 `explorer-ops-b11` 절
//! (refused requests · retry of originals)을 옮겼다. 그리기는 본체와 같은 공용 위젯이 맡는다.

use tasty_type_appearance::theme::Theme;
use tasty_type_appearance::toast_kind::ToastKind;
use tasty_ui_widgets::{
    ButtonVariant, OpStatusProps, ResultAction, ResultCardProps, ResultLine, op_status_line,
    result_card,
};

use super::{BODY_H, cell, glyph_painter};
use crate::catalog::icons;
use crate::catalog::spec::{self, StageVariant, TokenChip};
use crate::i18n::{t, t_args, t_count, t_fmt};

/// 받지 않은 요청: 대기열이 찼을 때(작업이 돌고 있어 Show queue 를 붙인다)와 요청이 너무 클 때.
/// 본체처럼 칸 안 상태줄 위에 경고 카드로 뜬다.
pub fn draw_refused(ui: &mut egui::Ui, theme: &Theme) {
    let close = glyph_painter(icons::CLOSE);
    let w = theme.toast_max_width().value();
    let sm = theme.spacing_sm.value();
    let copying = t_args("explorer.op.copying", &["3", "40", "photos/2026-09"]);
    let queued = t_fmt("explorer.op.queued", "8");
    let show_queue = [ResultAction {
        label: t("explorer.request.show_queue"),
        variant: ButtonVariant::Ghost,
    }];
    spec::stage(ui, theme, StageVariant::Column, |ui| {
        ui.spacing_mut().item_spacing.y = theme.spacing_lg.value();
        for (text, actions, running) in [
            (t("explorer.request.queue_full"), &show_queue[..], true),
            (t("explorer.request.too_large"), &[][..], false),
        ] {
            let (rect, status) = cell(ui, theme, BODY_H);
            if running {
                op_status_line(
                    ui,
                    theme,
                    status,
                    &OpStatusProps {
                        fraction: Some(0.2),
                        waiting: false,
                        text: &copying,
                        bytes: None,
                        queued: Some(&queued),
                        show_label: t("explorer.op.show"),
                        cancel_tip: t("explorer.op.cancel"),
                    },
                    &close,
                );
            }
            let card_rect = egui::Rect::from_min_max(
                egui::pos2(rect.right() - sm - w, rect.top() + sm),
                egui::pos2(rect.right() - sm, status.top() - sm),
            );
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(card_rect));
            result_card(
                &mut child,
                theme,
                w,
                &ResultCardProps {
                    kind: ToastKind::Warning,
                    title: text,
                    lines: &[],
                    more: None,
                    actions,
                    dismiss_tip: t("explorer.result.dismiss"),
                },
                &close,
            );
        }
    });
    spec::meta(
        ui,
        theme,
        &[
            (
                "where",
                "inside the cell, above the status line · not a result",
            ),
            ("time", "standard toast time · the same reason extends it"),
            (
                "queue full",
                "explorer.request.queue_full · Show queue while a job runs",
            ),
            ("too large", "explorer.request.too_large · no action"),
        ],
        &[TokenChip::new(
            "toast-accent-warning",
            "both",
            theme.toast_accent_warning().to_egui(),
        )],
    );
}

/// 남은 원본 다시 지우기: 비교·지우기 진행, 다 지운 성공 카드, 네 사유로 남긴 경고 카드.
pub fn draw_remove_originals(ui: &mut egui::Ui, theme: &Theme) {
    let close = glyph_painter(icons::CLOSE);
    let w = theme.toast_max_width().value();
    let checking = t_args("explorer.progress.checking", &["3", "12", "photos/2026-09"]);
    let removing = t_args(
        "explorer.progress.removing_originals",
        &["5", "12", "archive.zip"],
    );
    let removed = t_count("explorer.result.removed_originals", 12, &["12"]);
    let left_title = t_count("explorer.result.source_left_move", 4, &["40", "40", "4"]);
    let kept = t_count("explorer.result.kept_not_in_copy", 3, &["3"]);
    let lines = [
        ResultLine {
            path: "~/Volumes/usb/notes.md",
            reason: t("explorer.result.copy_missing"),
        },
        ResultLine {
            path: "~/Volumes/usb/photos",
            reason: &kept,
        },
        ResultLine {
            path: "~/Volumes/usb/mnt-link",
            reason: t("explorer.result.same_as_copy"),
        },
        ResultLine {
            path: "~/Volumes/usb/archive",
            reason: t("explorer.result.remove_cancelled"),
        },
    ];
    let retry4 = t_fmt("explorer.result.retry", "4");
    let actions = [
        ResultAction {
            label: &retry4,
            variant: ButtonVariant::Secondary,
        },
        ResultAction {
            label: t("explorer.result.copy_paths"),
            variant: ButtonVariant::Ghost,
        },
    ];
    spec::stage(ui, theme, StageVariant::Column, |ui| {
        ui.spacing_mut().item_spacing.y = theme.spacing_sm.value();
        for text in [&checking, &removing] {
            let (_, status) = cell(ui, theme, BODY_H);
            op_status_line(
                ui,
                theme,
                status,
                &OpStatusProps {
                    fraction: None,
                    waiting: false,
                    text,
                    bytes: None,
                    queued: None,
                    show_label: t("explorer.op.show"),
                    cancel_tip: t("explorer.op.cancel"),
                },
                &close,
            );
        }
        for (kind, title, lines, actions) in [
            (ToastKind::Success, removed.as_str(), &[][..], &[][..]),
            (
                ToastKind::Warning,
                left_title.as_str(),
                &lines[..],
                &actions[..],
            ),
        ] {
            result_card(
                ui,
                theme,
                w,
                &ResultCardProps {
                    kind,
                    title,
                    lines,
                    more: None,
                    actions,
                    dismiss_tip: t("explorer.result.dismiss"),
                },
                &close,
            );
        }
    });
    spec::meta(
        ui,
        theme,
        &[
            (
                "progress",
                "Checking while comparing · Removing originals while deleting",
            ),
            ("queue", "explorer.queue.remove_originals"),
            ("success", "Removed n originals · standard time · no Undo"),
            (
                "kept",
                "source_left_move · stays · Retry n · Copy paths · one card per press",
            ),
            (
                "reasons",
                "kept_not_in_copy · copy_missing · same_as_copy · remove_cancelled",
            ),
        ],
        &[
            TokenChip::new(
                "toast-accent-success",
                "removed",
                theme.toast_accent_success().to_egui(),
            ),
            TokenChip::new(
                "toast-accent-warning",
                "kept",
                theme.toast_accent_warning().to_egui(),
            ),
        ],
    );
}
