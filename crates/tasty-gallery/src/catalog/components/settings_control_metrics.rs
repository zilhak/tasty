//! 설정 컨트롤 치수 예제 — 단축키 행의 녹화 버튼·행 간격과 드래그 반전 modifier 행을 Mocha·Latte 로 나란히 둔다.
//!
//! 행은 본체 Keybindings › General 서브탭의 실제 필드와 기본 바인딩이다. 드래그 반전 행은 본체처럼
//! 엔트리 끝 구분선 뒤에 둔다. Category switch modifier 는 본체에서 Workspace 서브탭에 있어 이 무대에
//! 넣지 않는다. 행·슬롯·반전 행 그리기는 바인딩 행 예제와 공유한다.

use tasty_settings::KeybindingSettings;
use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{SettingsRow, settings_label_column, vspace};

use super::settings_keybinding_rows::{drag_flip, drag_flip_row, row};
use super::settings_task_pipeline::separator_line;
use crate::catalog::modifier_label;
use crate::catalog::spec::{self, StageVariant, TokenChip};
use crate::catalog::widgets::dialog as kit;
use crate::i18n::t;

/// 한 테마 프레임의 폭(시안 표본 480).
const WIDTH: LogicalPx = LogicalPx(480.0);

/// General 서브탭에서 이어진 세 필드(본체 순서) — 바인딩 없음, 둘, 하나.
const FIELDS: [&str; 3] = [
    "open_file_picker",
    "toggle_command_palette",
    "toggle_sidebar",
];

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    let with_zoom =
        |base: Theme| Theme::with_colors_and_zoom(base.to_colors(), base.is_light, theme.ui_zoom);
    let themes = [
        ("Mocha", with_zoom(tasty_themes::mocha_fallback())),
        ("Latte", with_zoom(crate::host_shell::latte_theme())),
    ];
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        for (i, (label, th)) in themes.iter().enumerate() {
            spec::wrap_item(ui, |ui| {
                ui.push_id(i, |ui| frame(ui, th, label));
            });
        }
    });

    spec::meta(
        ui,
        theme,
        &[
            (
                "record button",
                "kb-record-width 140 × kb-record-height 24 · mono caption · surface-raised · 1px border-default",
            ),
            (
                "add (+)",
                "kb-record-add-width 32 × 24 · None slot when empty",
            ),
            ("in-row gap", "space-xs 4"),
            ("row gap", "kb-row-gap 8 · no divider"),
            (
                "drag flip row",
                "Keybindings › General · after the entries, past a separator · Select field-width-md · caption · options of this OS, default first",
            ),
            (
                "font combo",
                "list max font-combo-list-max-height 300 · search field = list − 2 × font-combo-search-inset 4 · borders = Select: closed select-border, open select-border-focus (also the switch modifier combos)",
            ),
            ("open Select", "trigger border select-border-focus"),
        ],
        &[
            TokenChip::without_color("kb-record-width", "→ kb-ie-slot-min-width 140"),
            TokenChip::without_color("kb-record-height", "→ kb-ie-slot-height 24"),
            TokenChip::without_color("kb-record-add-width", "→ size-32"),
            TokenChip::without_color("kb-row-gap", "→ space-sm 8"),
            TokenChip::without_color("font-combo-list-max-height", "→ size-300"),
            TokenChip::without_color("font-combo-search-inset", "→ space-xs 4"),
            TokenChip::new(
                "select-border-focus",
                "open Select",
                theme.select_border_focus().to_egui(),
            ),
        ],
    );

    spec::note(
        ui,
        theme,
        "Rows are the app's General fields with their default bindings. The Category switch \
         modifier lives in the app's Workspace subtab, so the General stage ends with the drag \
         flip row alone.",
    );
}

/// 한 테마의 General 서브탭 — 바인딩 행, 구분선, 드래그 반전 행.
fn frame(ui: &mut egui::Ui, th: &Theme, label: &str) {
    let defaults = KeybindingSettings::default();
    let rows: Vec<(&str, Vec<String>)> = FIELDS
        .iter()
        .map(|field| {
            let label_key = KeybindingSettings::binding_fields()
                .find(|(id, _)| id == field)
                .map_or(*field, |(_, key)| *key);
            let bindings = defaults
                .get_bindings(field)
                .unwrap_or_default()
                .iter()
                .map(|combo| modifier_label(combo))
                .collect();
            (t(label_key), bindings)
        })
        .collect();
    kit::frame_card_flat(ui, th, WIDTH, kit::panel_fill(th), |ui| {
        kit::region_sym(ui, th.spacing_lg, th.spacing_md, |ui| {
            ui.spacing_mut().item_spacing.y = th.kb_row_gap().value();
            ui.label(
                egui::RichText::new(format!("{label} · Keybindings › General"))
                    .size(th.font_size_caption.value())
                    .color(th.text_muted().to_egui()),
            );
            let flip = drag_flip_row();
            let measured: Vec<SettingsRow<'_>> = rows
                .iter()
                .map(|(label, _)| SettingsRow::new(label))
                .chain([flip])
                .collect();
            let col = settings_label_column(ui, th, &measured);
            for (i, (label, bindings)) in rows.iter().enumerate() {
                let bindings: Vec<&str> = bindings.iter().map(String::as_str).collect();
                ui.push_id(i, |ui| row(ui, th, col, label, None, &bindings));
            }
            vspace(ui, th.spacing_sm);
            separator_line(ui, th);
            vspace(ui, th.spacing_xs);
            drag_flip(ui, th, col, flip);
        });
    });
}
