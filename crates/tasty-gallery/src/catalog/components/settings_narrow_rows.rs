//! 좁은 폭의 설정 행 — 시안 `overlays-windows-b12.jsx` 의 "Settings rows at narrow widths".
//!
//! 같은 네 행을 창 폭 700 · 520 · 420(사이드바 200, 좌우 inset 16 을 뺀 콘텐츠 폭)에서 그린다. 컨트롤이
//! 라벨 옆에 들어가지 않는 행만 라벨 아래 행 시작 x 로 쌓이고, 들어가는 행은 나란히 남는다. 쌓을지는
//! 본체와 같은 `settings_stack_row` 가 정하므로 프레임 폭만 다르게 준다.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{Input, SettingsRow, select, settings_label_column};

use crate::catalog::spec::{self, StageVariant, TokenChip};
use crate::catalog::widgets::dialog as kit;

/// 시안의 창 폭 셋.
const WINDOW_WIDE: LogicalPx = LogicalPx(700.0);
const WINDOW_MID: LogicalPx = LogicalPx(520.0);
const WINDOW_NARROW: LogicalPx = LogicalPx(420.0);
const WINDOWS: [LogicalPx; 3] = [WINDOW_WIDE, WINDOW_MID, WINDOW_NARROW];
/// 시안 `WSettingsCol` 의 사이드바 폭과 콘텐츠 좌우 inset — 창 폭에서 콘텐츠 폭을 구한다.
const SIDEBAR: LogicalPx = LogicalPx(200.0);
const INSET: LogicalPx = LogicalPx(16.0);

const NEXT: &str = "Next workspace";
const MODIFIER: &str = "Quick switch modifier";
const SCRIPT: &str = "Run \u{201c}format-buffer.lua\u{201d}";

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        for win in WINDOWS {
            let content = win - SIDEBAR - INSET - INSET;
            spec::wrap_item(ui, |ui| {
                // 행의 쌓임 상태는 Ui id 로 기억하므로 견본 칸마다 id 를 나눈다.
                ui.push_id(("narrow_rows", win.value() as i32), |ui| {
                    ui.spacing_mut().item_spacing.y = theme.spacing_xs.value();
                    ui.label(
                        egui::RichText::new(format!(
                            "window {} · content {}",
                            win.value(),
                            content.value()
                        ))
                        .size(theme.font_size_caption.value())
                        .color(theme.text_muted().to_egui()),
                    );
                    let outer = content + theme.spacing_md + theme.spacing_md;
                    kit::frame_card_flat(ui, theme, outer, kit::panel_fill(theme), |ui| {
                        kit::region_sym(ui, theme.spacing_md, theme.spacing_md, |ui| {
                            column(ui, theme);
                        });
                    });
                });
            });
        }
    });
    spec::meta(
        ui,
        theme,
        &[
            (
                "switch",
                "per Row · control natural width > content − label column − settings-label-gap",
            ),
            (
                "stacked",
                "label full width · control at row start · gap settings-row-stack-gap",
            ),
            (
                "back",
                "only when it fits with settings-row-stack-hysteresis spare",
            ),
            (
                "inside",
                "existing wraps (buttons from control x · unit under the field)",
            ),
        ],
        &[
            TokenChip::without_color("settings-row-stack-gap", "→ space-xs"),
            TokenChip::without_color("settings-row-stack-hysteresis", "→ space-lg"),
        ],
    );
}

fn column(ui: &mut egui::Ui, theme: &Theme) {
    ui.spacing_mut().item_spacing.y = theme.settings_row_gap().value();
    let rows = [
        SettingsRow::new(NEXT),
        SettingsRow::new(MODIFIER),
        SettingsRow::new(SCRIPT),
        SettingsRow::new(crate::i18n::t("settings.remote_transfer.max_capacity")),
    ];
    let col = settings_label_column(ui, theme, &rows);
    let [_, modifier, _, max_size] = rows;
    super::settings_keybinding_rows::row(ui, theme, col, NEXT, None, &["Ctrl+Alt+Down"]);
    let mut selected = 0;
    modifier.show(ui, theme, col, |ui| {
        select(
            ui,
            theme,
            "gallery_narrow_modifier",
            &mut selected,
            &["Ctrl+Alt"],
            theme.field_width_md.value(),
            true,
        );
    });
    super::settings_keybinding_rows::row(ui, theme, col, SCRIPT, None, &[]);
    max_size.show(ui, theme, col, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = theme.spacing_xs.value();
            let mut value = String::from("512");
            Input::new()
                .mono(true)
                .width(theme.field_width_xs.value())
                .show(ui, theme, &mut value);
            ui.label(
                egui::RichText::new("MB")
                    .size(theme.font_size_caption.value())
                    .color(theme.text_muted().to_egui()),
            );
        });
    });
}
