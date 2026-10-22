//! 성능 탭 예제. 실제 설정 저장은 하지 않는다.
//!
//! 탭 맨 위에 적용 시점 안내(info 글리프 + caption)를 한 번 두고, 본체와 같은 settings-row 그리드에
//! 스위치 두 행과 숫자 행 하나를 둔다. 디스크 보관 줄 수는 디스크 스왑이 꺼져 있으면 비활성이고,
//! -1 이 무제한이라 범위 하한이며 그 값이면 칸 뒤에 "no limit" 을 적는다.

use std::cell::RefCell;

use tasty_settings::{DEFAULT_SCROLLBACK_DISK_LINES, UNLIMITED_SCROLLBACK_DISK_LINES};
use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{Input, SettingsRow, settings_apply_note, settings_label_column};

use crate::catalog::spec::{self, StageVariant, TokenChip};
use crate::catalog::widgets::dialog as kit;

use super::settings_remote_transfer::WIDTH;

/// 본체 `tabs/performance.rs` 의 상한과 같은 값이다.
const DISK_LINES_MAX: i64 = 100_000_000;
/// 디자인 견본의 좁은 폭(size-360). 안내가 줄을 바꾸고 행이 라벨 아래로 쌓인다.
const NARROW: LogicalPx = LogicalPx(360.0);

struct Case {
    label: &'static str,
    latte: bool,
    width: LogicalPx,
    polling: bool,
    disk_swap: bool,
    disk_lines: String,
}

fn case(label: &'static str, latte: bool, disk_swap: bool, lines: i64) -> Case {
    Case {
        label,
        latte,
        width: WIDTH,
        polling: true,
        disk_swap,
        disk_lines: lines.to_string(),
    }
}

thread_local! {
    static STATE: RefCell<Vec<Case>> = RefCell::new(vec![
        case("Mocha · default · disk swap on", false, true, DEFAULT_SCROLLBACK_DISK_LINES),
        case("Latte · default · disk swap on", true, true, DEFAULT_SCROLLBACK_DISK_LINES),
        case("Mocha · disk swap off · lines disabled", false, false, DEFAULT_SCROLLBACK_DISK_LINES),
        case("Latte · -1 · no limit", true, true, UNLIMITED_SCROLLBACK_DISK_LINES),
        case("Mocha · larger config value, shown as read", false, true, 2_500_000),
        Case {
            width: NARROW,
            ..case("Latte · narrow 360 · disk swap off · -1", true, false, UNLIMITED_SCROLLBACK_DISK_LINES)
        },
    ]);
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    let with_zoom =
        |base: Theme| Theme::with_colors_and_zoom(base.to_colors(), base.is_light, theme.ui_zoom);
    let mocha = with_zoom(tasty_themes::mocha_fallback());
    let latte = with_zoom(crate::host_shell::latte_theme());
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        STATE.with(|s| {
            for (i, case) in s.borrow_mut().iter_mut().enumerate() {
                let th = if case.latte { &latte } else { &mocha };
                spec::wrap_item(ui, |ui| {
                    ui.push_id(i, |ui| frame(ui, th, case));
                });
            }
        });
    });

    spec::meta(
        ui,
        theme,
        &[
            (
                "note",
                "top of the tab, above the rows · info glyph icon-size-sm · caption · wraps at measure-md",
            ),
            (
                "rows",
                "polling switch · disk swap switch · lines kept on disk",
            ),
            (
                "field",
                "mono Input · field-width-color 110 · right-aligned · unit addon",
            ),
            (
                "-1",
                "no limit after the field (state-disabled while swap is off)",
            ),
            ("range", "-1 (unlimited) … 100000000 · clamp on commit"),
            ("default value", "10000"),
            ("enabled when", "disk swap is on"),
        ],
        &[
            TokenChip::new(
                "settings-apply-note-fg",
                "→ text-secondary",
                theme.settings_apply_note_fg().to_egui(),
            ),
            TokenChip::new(
                "settings-apply-note-glyph",
                "→ text-muted",
                theme.settings_apply_note_glyph().to_egui(),
            ),
            TokenChip::without_color("settings-apply-note-gap", "→ space-sm 8"),
            TokenChip::without_color("settings-row-gap", "→ space-md 12"),
            TokenChip::without_color("field-width-color", "110"),
            TokenChip::new(
                "accent-danger",
                "out of range",
                theme.accent_danger().to_egui(),
            ),
        ],
    );
}

fn frame(ui: &mut egui::Ui, th: &Theme, case: &mut Case) {
    use crate::i18n::t;
    kit::frame_card_flat(ui, th, case.width, kit::panel_fill(th), |ui| {
        kit::region_sym(ui, th.spacing_lg, th.spacing_md, |ui| {
            ui.spacing_mut().item_spacing.y = th.settings_row_gap().value();
            ui.label(
                egui::RichText::new(case.label)
                    .size(th.font_size_caption.value())
                    .color(th.text_muted().to_egui()),
            );
            settings_apply_note(ui, th, t("settings.performance.apply_notice"));
            let rows = [
                SettingsRow::new(t("settings.performance.targeted_pty_polling"))
                    .hint(t("settings.performance.targeted_pty_polling_desc")),
                SettingsRow::new(t("settings.performance.scrollback_disk_swap"))
                    .hint(t("settings.performance.scrollback_disk_swap_desc")),
                SettingsRow::new(t("settings.performance.scrollback_disk_lines"))
                    .hint(t("settings.performance.scrollback_disk_lines_desc")),
            ];
            let col = settings_label_column(ui, th, &rows);
            let [polling, disk_swap, disk_lines] = rows;
            polling.show(ui, th, col, |ui| {
                tasty_ui_widgets::switch(ui, th, &mut case.polling, None, true);
            });
            disk_swap.show(ui, th, col, |ui| {
                tasty_ui_widgets::switch(ui, th, &mut case.disk_swap, None, true);
            });
            let enabled = case.disk_swap;
            disk_lines.show(ui, th, col, |ui| {
                lines_control(ui, th, &mut case.disk_lines, enabled)
            });
        });
    });
}

/// 숫자 Input 과 단위 addon, -1 이면 칸 뒤 "no limit", 범위 밖이면 그 아래 danger 한 줄
/// (본체 `number_field` 와 같은 자리).
fn lines_control(ui: &mut egui::Ui, theme: &Theme, buf: &mut String, enabled: bool) {
    use crate::i18n::t;
    let (min, max) = (UNLIMITED_SCROLLBACK_DISK_LINES, DISK_LINES_MAX);
    let parsed = buf.trim().parse::<i64>().ok();
    let settled = parsed
        .map(|v| (v, v.clamp(min, max)))
        .and_then(|(v, c)| (v != c).then_some(c));
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = theme.spacing_xs.value();
        ui.horizontal_wrapped(|ui| {
            Input::new()
                .mono(true)
                .align(egui::Align::RIGHT)
                .width(theme.field_width_color.value())
                .addon(t("settings.number.unit_lines"))
                .enabled(enabled)
                .invalid(settled.is_some())
                .show(ui, theme, buf);
            if parsed == Some(UNLIMITED_SCROLLBACK_DISK_LINES) {
                ui.add_space((theme.spacing_sm.value() - ui.spacing().item_spacing.x).max(0.0));
                let ink = if enabled {
                    theme.text_secondary()
                } else {
                    theme.state_disabled_fg()
                };
                ui.label(
                    egui::RichText::new(t("settings.performance.no_limit"))
                        .size(theme.font_size_caption.value())
                        .color(ink.to_egui()),
                );
            }
        });
        if let Some(settled) = settled {
            let line = t("settings.number.range_between")
                .replacen("{}", &min.to_string(), 1)
                .replacen("{}", &max.to_string(), 1)
                .replacen("{}", &settled.to_string(), 1);
            ui.label(
                egui::RichText::new(line)
                    .size(theme.font_size_caption.value())
                    .color(theme.accent_danger().to_egui()),
            );
        }
    });
}
