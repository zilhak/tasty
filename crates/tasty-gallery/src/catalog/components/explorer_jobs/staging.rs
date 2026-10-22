//! 준비용 임시 폴더(`.tasty-copy-*`)를 지우지 못한 결과 카드 — 디자인 `gallery/explorer-ops-staging.jsx`
//! (#stagingleft) 예제를 옮겼다. 종류는 항목 결과와 Warning 중 높은 쪽, 제목은 항목 결과 그대로이며
//! 남은 폴더 줄은 항목 줄 뒤에 온다. 동작 버튼은 본체 `card_actions` 와 같다.

use tasty_type_appearance::theme::Theme;
use tasty_type_appearance::toast_kind::ToastKind;
use tasty_ui_widgets::{ButtonVariant, ResultAction, ResultCardProps, ResultLine, result_card};

use super::glyph_painter;
use crate::catalog::icons;
use crate::catalog::spec::{self, StageVariant, TokenChip};
use crate::i18n::{t, t_args, t_fmt, t_fmt2};

pub fn draw_staging_left(ui: &mut egui::Ui, theme: &Theme) {
    let close = glyph_painter(icons::CLOSE);
    let w = theme.toast_max_width().value();
    let card = |ui: &mut egui::Ui,
                title: &str,
                lines: &[ResultLine<'_>],
                more: Option<&str>,
                actions: &[ResultAction<'_>]| {
        result_card(
            ui,
            theme,
            w,
            &ResultCardProps {
                kind: ToastKind::Warning,
                title,
                body: None,
                lines,
                more,
                actions,
                dismiss_tip: t("explorer.result.dismiss"),
            },
            &close,
        );
    };
    let left = |error: &str| t_fmt("explorer.result.staging_left", error);
    let denied = left("Permission denied (os error 13)");
    let not_empty = left("Directory not empty (os error 39)");
    let not_removed = t_fmt(
        "explorer.result.source_not_removed",
        "Device or resource busy (os error 16)",
    );
    let copy_paths = || ResultAction {
        label: t("explorer.result.copy_paths"),
        variant: ButtonVariant::Ghost,
    };
    let undo = ResultAction {
        label: t("explorer.result.undo"),
        variant: ButtonVariant::Ghost,
    };
    let retry28 = t_fmt("explorer.result.retry", "28");
    let retry3 = t_fmt("explorer.result.retry", "3");
    let retry = |label| ResultAction {
        label,
        variant: ButtonVariant::Secondary,
    };
    spec::stage(ui, theme, StageVariant::Column, |ui| {
        ui.spacing_mut().item_spacing.y = theme.spacing_sm.value();
        card(
            ui,
            &t_fmt2("explorer.result.copied", "2", "Documents"),
            &[ResultLine {
                path: "~/Documents/.tasty-copy-AbC123",
                reason: &denied,
            }],
            None,
            &[copy_paths(), undo],
        );
        // 취소 카드의 Retry 는 하지 않은 28개를 센다. 남은 폴더는 Retry 대상이 아니다.
        card(
            ui,
            &t_fmt2("explorer.result.cancelled_copy", "12", "40"),
            &[ResultLine {
                path: "~/Documents/.tasty-copy-Q7x2Lm",
                reason: &not_empty,
            }],
            None,
            &[retry(&retry28), copy_paths()],
        );
        // 항목 줄 셋이 줄 상한을 채우므로 남은 폴더 줄은 "and 1 more" 에 든다.
        card(
            ui,
            &t_args("explorer.result.partial_move", &["37", "40", "3"]),
            &[
                ResultLine {
                    path: "~/Downloads/build/app-release-unsigned.apk",
                    reason: "Permission denied (os error 13)",
                },
                ResultLine {
                    path: "~/Downloads/archive.zip",
                    reason: &not_removed,
                },
                ResultLine {
                    path: "~/Downloads/raw/IMG_2031.heic",
                    reason: "No space left on device (os error 28)",
                },
            ],
            Some(&t_fmt("explorer.result.more", "1")),
            &[retry(&retry3), copy_paths()],
        );
        card(
            ui,
            t("explorer.result.undo_done_copy"),
            &[ResultLine {
                path: "~/Downloads/.tasty-copy-u5Rt8W",
                reason: &denied,
            }],
            None,
            &[copy_paths()],
        );
    });
    spec::meta(
        ui,
        theme,
        &[
            (
                "kind",
                "max(items' kind, Warning) · Success / Info → Warning · Danger stays",
            ),
            ("title", "items' result, unchanged"),
            (
                "line",
                "folder path + explorer.result.staging_left (OS error in ( ))",
            ),
            ("order", "after item lines · counts toward “and n more”"),
            (
                "actions",
                "Copy paths (+ folder) · Undo if allowed · Retry never counts the folder",
            ),
            ("lifetime", "until closed"),
        ],
        &[
            TokenChip::new(
                "toast-accent-warning",
                "all four",
                theme.toast_accent_warning().to_egui(),
            ),
            TokenChip::without_color("toast-max-width", "320"),
        ],
    );
}
