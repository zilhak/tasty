//! Keybindings 서브탭(General ~ Scripts)의 바인딩 행 예제. 실제 녹화는 하지 않는다.
//!
//! 라벨 열은 서브탭이 공유하는 고정 폭이고, 라벨이 짧아도 열을 그대로 차지해 바인딩 버튼이 같은 x 에서
//! 시작한다. 열보다 긴 이름(사용자 스크립트)은 열 안에서 줄을 바꾼다.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{settings_label_cell, settings_label_gap};

use crate::catalog::spec::{self, StageVariant, TokenChip};
use crate::catalog::widgets::dialog as kit;

/// 본체 단축키 탭이 공유하는 라벨 열 폭(`keybindings_tab::LABEL_COL_WIDTH`).
const LABEL_COL: LogicalPx = LogicalPx(288.0);
/// 본체 바인딩 버튼의 폭·높이(`entries.rs`).
const SLOT_W: LogicalPx = LogicalPx(140.0);
const SLOT_H: LogicalPx = LogicalPx(24.0);
const ADD_W: LogicalPx = LogicalPx(32.0);
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
        "deploy-staging-and-rotate-every-log-on-all-the-hosts-tonight.lua",
        None,
        &["Ctrl+Alt+1"],
    ),
];

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        kit::frame_card_flat(ui, theme, WIDTH, kit::panel_fill(theme), |ui| {
            kit::region_sym(ui, theme.spacing_lg, theme.spacing_md, |ui| {
                ui.spacing_mut().item_spacing.y = theme.spacing_xs.value();
                for (i, (label, hint, bindings)) in ROWS.iter().enumerate() {
                    ui.push_id(i, |ui| row(ui, theme, label, *hint, bindings));
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
                "288 shared by General … Scripts · holds its width",
            ),
            ("gap", "16 label → first slot"),
            ("hint", "HelpHint inside the label column, after the label"),
            ("long name", "wraps inside the column (script names)"),
            (
                "slot",
                "140 × 24 mono · + (32) adds a slot · None when empty",
            ),
        ],
        &[
            TokenChip::without_color("settings-label-gap", "label → slot"),
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

fn row(ui: &mut egui::Ui, theme: &Theme, label: &str, hint: Option<&str>, bindings: &[&str]) {
    ui.horizontal_top(|ui| {
        settings_label_cell(ui, theme, LABEL_COL, SLOT_H, label, hint);
        settings_label_gap(ui, theme);
        ui.horizontal_wrapped(|ui| {
            let gap = theme.spacing_xs.value();
            ui.spacing_mut().item_spacing = egui::vec2(gap, gap);
            for combo in bindings {
                slot(ui, theme, combo, theme.text_primary().to_egui(), SLOT_W);
            }
            if bindings.is_empty() {
                slot(ui, theme, "None", theme.text_muted().to_egui(), SLOT_W);
            } else {
                slot(ui, theme, "+", theme.text_muted().to_egui(), ADD_W);
            }
        });
    });
}

fn slot(ui: &mut egui::Ui, theme: &Theme, text: &str, color: egui::Color32, width: LogicalPx) {
    ui.add(
        egui::Button::new(egui::RichText::new(text).monospace().color(color))
            .fill(theme.surface_raised().to_egui())
            .min_size(egui::vec2(width.value(), SLOT_H.value())),
    );
}
