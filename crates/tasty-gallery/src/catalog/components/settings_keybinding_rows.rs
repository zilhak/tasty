//! Keybindings 서브탭(General ~ Scripts)의 바인딩 행 예제. 실제 녹화는 하지 않는다.
//!
//! 라벨 열은 다른 설정 행과 같이 서브탭의 가장 긴 라벨을 150 … 240 으로 clamp 한 폭이다. 라벨이 짧아도
//! 열을 그대로 차지해 바인딩 버튼이 같은 x 에서 시작한다. 열보다 긴 라벨과 사용자 스크립트 이름은 열
//! 안에서 줄을 바꾸고, 도움말 아이콘은 마지막 단어 뒤에 붙는다.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{
    SettingsRow, settings_label_cell, settings_label_column, settings_label_gap,
};

use crate::catalog::spec::{self, StageVariant, TokenChip};
use crate::catalog::widgets::dialog as kit;

/// 예제 프레임 폭 — 설정 콘텐츠 컬럼 상한(620)을 넘지 않는다.
const WIDTH: LogicalPx = LogicalPx(600.0);

/// (라벨, 도움말, 바인딩들). 바인딩이 없으면 "None" 슬롯을 그린다.
const ROWS: &[(&str, Option<&str>, &[&str])] = &[
    ("New workspace:", None, &["Ctrl+Shift+N"]),
    (
        "Close active:",
        Some("Closes in order: tab → pane → workspace."),
        &["Ctrl+W"],
    ),
    ("Quit:", None, &["Ctrl+Q", "Ctrl+Shift+Q"]),
    ("アクティブ項目を閉じる:", None, &[]),
    (
        "Explorer: reveal the current path in the folder tree:",
        Some("Expands the tree down to the folder of the focused surface."),
        &["Ctrl+Alt+E"],
    ),
    (
        "deploy-staging-and-rotate-every-log-on-all-the-hosts-tonight.lua",
        None,
        &["Ctrl+Alt+1"],
    ),
];

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        kit::frame_card_flat(ui, theme, WIDTH, kit::panel_fill(theme), |ui| {
            kit::region_sym(ui, theme.spacing_lg, theme.spacing_md, |ui| {
                // 행 사이는 kb-row-gap, 구분선 없음. 행 안 버튼 사이는 space-xs 다.
                ui.spacing_mut().item_spacing.y = theme.kb_row_gap().value();
                let measured: Vec<SettingsRow<'_>> = ROWS
                    .iter()
                    .map(|(label, hint, _)| {
                        let row = SettingsRow::new(label);
                        match hint {
                            Some(h) => row.hint(h),
                            None => row,
                        }
                    })
                    .collect();
                let col = settings_label_column(ui, theme, &measured);
                for (i, (label, hint, bindings)) in ROWS.iter().enumerate() {
                    ui.push_id(i, |ui| row(ui, theme, col, label, *hint, bindings));
                }
            });
        });
    });

    spec::meta(
        ui,
        theme,
        &[
            (
                "label column",
                "longest label of the subtab, clamp 150 … 240 · holds its width",
            ),
            ("gap", "16 label → first slot"),
            (
                "hint",
                "HelpHint inside the label column, after the last word",
            ),
            (
                "long label",
                "wraps inside the column (long labels, script names)",
            ),
            (
                "slot",
                "kb-record-width 140 × kb-record-height 24 mono · + (kb-record-add-width 32) adds a slot · None when empty",
            ),
            (
                "rows",
                "kb-row-gap 8 between rows · no divider · 4 between slots",
            ),
        ],
        &[
            TokenChip::without_color("settings-label-width", "label column floor 150"),
            TokenChip::without_color("settings-label-max-width", "label column cap 240"),
            TokenChip::without_color("settings-label-gap", "label → slot"),
            TokenChip::without_color("kb-record-width", "→ kb-ie-slot-min-width 140"),
            TokenChip::without_color("kb-record-height", "→ kb-ie-slot-height 24"),
            TokenChip::without_color("kb-record-add-width", "→ size-32"),
            TokenChip::without_color("kb-row-gap", "→ space-sm 8"),
            TokenChip::new("text-secondary", "label", theme.text_secondary().to_egui()),
            TokenChip::new("surface-raised", "slot", theme.surface_raised().to_egui()),
        ],
    );

    spec::note(
        ui,
        theme,
        "Mirrors the app's keybinding rows. A short label no longer pulls its slots left: the label cell always takes the full column.",
    );
}

fn row(
    ui: &mut egui::Ui,
    theme: &Theme,
    col: LogicalPx,
    label: &str,
    hint: Option<&str>,
    bindings: &[&str],
) {
    ui.horizontal_top(|ui| {
        settings_label_cell(ui, theme, col, theme.kb_record_height(), label, hint);
        settings_label_gap(ui, theme);
        ui.horizontal_wrapped(|ui| {
            let gap = theme.spacing_xs.value();
            ui.spacing_mut().item_spacing = egui::vec2(gap, gap);
            for combo in bindings {
                slot(
                    ui,
                    theme,
                    combo,
                    theme.text_primary().to_egui(),
                    theme.kb_record_width(),
                );
            }
            if bindings.is_empty() {
                slot(
                    ui,
                    theme,
                    "None",
                    theme.text_muted().to_egui(),
                    theme.kb_record_width(),
                );
            } else {
                slot(
                    ui,
                    theme,
                    "+",
                    theme.text_muted().to_egui(),
                    theme.kb_record_add_width(),
                );
            }
        });
    });
}

fn slot(ui: &mut egui::Ui, theme: &Theme, text: &str, color: egui::Color32, width: LogicalPx) {
    ui.add(
        egui::Button::new(egui::RichText::new(text).monospace().color(color))
            .fill(theme.surface_raised().to_egui())
            .min_size(egui::vec2(width.value(), theme.kb_record_height().value())),
    );
}
