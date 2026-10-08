//! Settings › Appearance › Font override — 시안 Spec "Appearance › Font override"(`FontOverrideG`).
//! 행은 색 override 행과 같은 형태(라벨 · 자기 필드 폭의 컨트롤 · 뒤따르는 "Use default")이고,
//! 미리보기는 창 폭과 관계없이 격자 아래에 둔다.

use std::cell::RefCell;

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{
    Input, OverrideCell, SettingsRow, checkbox, override_row, select, settings_label_column,
};

use crate::catalog::icons::SEARCH;
use crate::catalog::spec::{StageVariant, TokenChip, meta, stage, wrap_item};

/// 시안 좁은 짝의 폭 `--tasty-size-360`. 공개 역할 토큰이 없어 갤러리 무대 치수로 둔다.
const NARROW_WIDTH: LogicalPx = LogicalPx(360.0);

/// 시안 행 seed — (라벨, 기본값을 따르는지).
const ROWS: [(&str, bool); 5] = [
    ("Font family", false),
    ("Custom font file", true),
    ("Font size", false),
    ("Line height", true),
    ("Font DPI scaling", true),
];

/// 본체 settings.appearance.*_tooltip 의 영어 문구.
const LINE_HEIGHT_HINT: &str =
    "Line height multiplier. 1.0 = tight (best for ASCII art), 1.2 = comfortable reading.";
const SCALE_MODE_HINT: &str = "Auto: Adjusts font rendering to match monitor DPI.\nSame physical \
     text size across different monitors.\n\nFixed: Uses the same pixel size regardless of DPI.\n\
     More cells on high-DPI monitors, smaller text.";

thread_local! {
    // 짝 둘 × 테마 둘 × 행 다섯의 (입력 버퍼, Use default) 상태.
    static STATE: RefCell<Option<Vec<(String, bool)>>> = const { RefCell::new(None) };
}

fn seed_value(row: usize) -> String {
    match row {
        0 => "D2Coding",
        2 => "14",
        3 => "1.2",
        _ => "",
    }
    .to_string()
}

fn control(ui: &mut egui::Ui, th: &Theme, row: usize, buf: &mut String, enabled: bool) {
    match row {
        0 => {
            Input::new()
                .enabled(enabled)
                .width(th.field_width_lg.value())
                .icon(&|ui, rect, c| SEARCH.image(rect.height(), c).paint_at(ui, rect))
                .show(ui, th, buf);
        }
        1 => {
            Input::new()
                .mono(true)
                .enabled(enabled)
                .placeholder("~/fonts/MyFont.ttf")
                .width(th.field_width_lg.value())
                .show(ui, th, buf);
        }
        2 | 3 => {
            let mut input = Input::new()
                .mono(true)
                .enabled(enabled)
                .width(th.field_width_xs.value());
            if row == 2 {
                input = input.addon("px");
            }
            input.show(ui, th, buf);
        }
        _ => {
            let mut idx = 0;
            select(
                ui,
                th,
                "font_override_dpi",
                &mut idx,
                &["Follow display"],
                th.field_width_md.value(),
                enabled,
            );
        }
    }
}

/// 미리보기 표본 — 라틴 · 한글 · 숫자 · 가나(본체 `draw_font_preview` 와 같은 네 줄).
const SAMPLE_LINES: [&str; 4] = [
    "AaBbCcDdEeFfGg",
    "가나다라마바사",
    "1234567890",
    "アカサタナハマラヤワ",
];

/// 미리보기 한 칸. 본체는 surface 의 실제 배경을 쓰고, 갤러리는 Focused `bg-app` · Unfocused
/// `bg-panel` 을 대역으로 쓴다. 글자색은 surface focused 전경색 대역 `text-primary`, 테두리는
/// Focused `border-strong` · Unfocused `separator`.
fn preview_block(ui: &mut egui::Ui, th: &Theme, focused: bool, w: f32) {
    ui.vertical(|ui| {
        ui.set_width(w);
        ui.spacing_mut().item_spacing.y = th.spacing_xs.value();
        ui.label(
            egui::RichText::new(if focused { "Focused" } else { "Unfocused" })
                .size(th.font_size_caption.value())
                .color(th.text_muted().to_egui()),
        );
        // separator 는 premultiplied 바이트로 저장된다.
        let (fill, border) = if focused {
            (th.bg_app().to_egui(), th.border_strong().to_egui())
        } else {
            (
                th.bg_panel().to_egui(),
                th.separator.to_egui_premultiplied(),
            )
        };
        let size = th.font_size_term.value();
        egui::Frame::new()
            .fill(fill)
            .stroke(egui::Stroke::new(th.border_width.value(), border))
            .corner_radius(th.corner_radius.value())
            .inner_margin(egui::Margin::symmetric(
                th.font_preview_padding_x().value() as i8,
                th.font_preview_padding_y().value() as i8,
            ))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = 0.0;
                for line in SAMPLE_LINES {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(line)
                                .monospace()
                                .size(size)
                                .line_height(Some(size * th.line_height_ui))
                                .color(th.text_primary().to_egui()),
                        )
                        .truncate(),
                    );
                }
            });
    });
}

fn font_override(ui: &mut egui::Ui, th: &Theme, long: bool, state: &mut [(String, bool)]) {
    let use_default = if long {
        "既定値を使用"
    } else {
        "Use default"
    };
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        // 본체처럼 line height · DPI scaling 라벨 뒤에 도움말 아이콘이 붙는다.
        let labels: Vec<SettingsRow<'_>> = ROWS
            .iter()
            .enumerate()
            .map(|(i, (label, _))| match i {
                3 => SettingsRow::new(label).hint(LINE_HEIGHT_HINT),
                4 => SettingsRow::new(label).hint(SCALE_MODE_HINT),
                _ => SettingsRow::new(label),
            })
            .collect();
        let col = settings_label_column(ui, th, &labels);
        let row_h = th.settings_row_min_height();
        for (i, (row, (buf, def))) in labels.iter().zip(state.iter_mut()).enumerate() {
            let control_w = match i {
                0 | 1 => th.field_width_lg,
                2 | 3 => th.field_width_xs,
                _ => th.field_width_md,
            };
            ui.push_id(i, |ui| {
                override_row(ui, th, col, control_w, use_default, |ui, cell| match cell {
                    OverrideCell::Label => {
                        row.show_label(ui, th, col, row_h);
                    }
                    OverrideCell::Control => control(ui, th, i, buf, !*def),
                    OverrideCell::UseDefault => {
                        checkbox(ui, th, def, use_default, true);
                    }
                });
            });
        }
        ui.add_space(th.spacing_lg.value());
        ui.spacing_mut().item_spacing.y = th.spacing_sm.value();
        ui.label(
            egui::RichText::new("Preview")
                .size(th.font_size_caption.value())
                .color(th.text_muted().to_egui()),
        );
        // 한 칸이 `font-preview-min-width` 보다 좁아지면 Unfocused 가 Focused 아래로 내려간다.
        let gap = th.spacing_md.value();
        let half = (ui.available_width() - gap) / 2.0;
        if half >= th.font_preview_min_width().value() {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                preview_block(ui, th, true, half);
                preview_block(ui, th, false, half);
            });
        } else {
            let w = ui.available_width();
            preview_block(ui, th, true, w);
            ui.add_space(gap - th.spacing_sm.value());
            preview_block(ui, th, false, w);
        }
        ui.label(
            egui::RichText::new("Font: D2Coding / 14.0px")
                .monospace()
                .size(th.font_size_caption.value())
                .color(th.text_muted().to_egui()),
        );
    });
}

fn theme_pair(
    ui: &mut egui::Ui,
    theme: &Theme,
    pair: usize,
    width: f32,
    state: &mut [(String, bool)],
) {
    let with_zoom =
        |base: Theme| Theme::with_colors_and_zoom(base.to_colors(), base.is_light, theme.ui_zoom);
    let themes = [
        ("Mocha", with_zoom(tasty_themes::mocha_fallback())),
        ("Latte", with_zoom(crate::host_shell::latte_theme())),
    ];
    // 시안 ThemePair 는 줄바꿈 줄이다. 두 타일이 칸에 안 들어가면 아래로 내려간다.
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing =
            egui::vec2(theme.spacing_lg.value(), theme.spacing_lg.value());
        for ((label, th), rows) in themes.iter().zip(state.chunks_mut(ROWS.len())) {
            wrap_item(ui, |ui| {
                egui::Frame::new()
                    .fill(th.bg_app().to_egui())
                    .corner_radius(th.corner_radius.value())
                    .inner_margin(egui::Margin::same(th.spacing_md.value() as i8))
                    .show(ui, |ui| {
                        ui.vertical(|ui| {
                            ui.set_width(width);
                            ui.spacing_mut().item_spacing.y = th.spacing_sm.value();
                            ui.label(
                                egui::RichText::new(*label)
                                    .size(th.font_size_caption.value())
                                    .color(th.text_muted().to_egui()),
                            );
                            ui.push_id(("font_override", pair, *label), |ui| {
                                font_override(ui, th, pair == 1, rows);
                            });
                        });
                    });
            });
        }
    });
}

/// Appearance › Font override — 행 격자와 그 아래 미리보기. 넓은 짝은 설정 본문 최대 폭,
/// 좁은 짝은 긴 일본어 체크박스 문구로 체크박스가 컨트롤 아래로 내려가는 경우다.
pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    STATE.with(|s| {
        let mut slot = s.borrow_mut();
        let state = slot.get_or_insert_with(|| {
            (0..4)
                .flat_map(|_| {
                    ROWS.iter()
                        .enumerate()
                        .map(|(i, (_, d))| (seed_value(i), *d))
                })
                .collect()
        });
        let (wide, narrow) = state.split_at_mut(ROWS.len() * 2);
        stage(ui, theme, StageVariant::Column, |ui| {
            ui.spacing_mut().item_spacing.y = theme.spacing_lg.value();
            theme_pair(
                ui,
                theme,
                0,
                theme.settings_content_max_width().value(),
                wide,
            );
            theme_pair(ui, theme, 1, NARROW_WIDTH.value(), narrow);
        });
    });

    meta(
        ui,
        theme,
        &[
            ("row", "label · control · Checkbox \"Use default\""),
            ("label", "settings-label-width · body · text-secondary"),
            (
                "control",
                "family · file → field-width-lg · size · line height → field-width-xs · DPI → field-width-md",
            ),
            (
                "Use default",
                "trailing, label always shown; checked disables the control",
            ),
            (
                "gaps",
                "column space-lg · row min-h settings-row-min-height · padding space-xs",
            ),
            (
                "narrow",
                "the checkbox drops under the control (row-gap space-xs)",
            ),
            (
                "preview",
                "below the grid · space-lg above · Focused / Unfocused flex 1 each",
            ),
            (
                "preview box (b2)",
                "the surface's effective bg (runtime colour, not a token) + effective font & size · 4 lines: latin · hangul · digits · kana · ink = surface focused fg · edge Focused border-strong / Unfocused separator · radius",
            ),
            ("padding", "font-preview-padding-y space-sm · -x space-md"),
            (
                "line height",
                "font-preview-line-height → line-height-ui 1.4 × effective size; box height follows (no height token)",
            ),
            (
                "stack",
                "a box narrower than font-preview-min-width (→ field-width-lg 200) wraps under the other — content width < 2 × 200 + space-md",
            ),
            ("summary", "mono caption · text-muted"),
        ],
        &[
            TokenChip::without_color("settings-label-width", "150 label column (new name)"),
            TokenChip::without_color("field-width-lg", "family · file"),
            TokenChip::without_color("field-width-md", "DPI"),
            TokenChip::without_color("field-width-xs", "numbers"),
            TokenChip::without_color("settings-row-min-height", "row"),
            TokenChip::without_color(
                "font-preview-min-width",
                "→ field-width-lg 200 · stack below",
            ),
            TokenChip::without_color("font-preview-padding-x", "→ space-md"),
            TokenChip::without_color("font-preview-padding-y", "→ space-sm"),
        ],
    );
}
