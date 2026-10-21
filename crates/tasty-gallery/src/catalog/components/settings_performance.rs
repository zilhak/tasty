//! 성능 탭 예제. 실제 설정 저장은 하지 않는다.
//!
//! 새 시각 값은 없다. 본체와 같은 settings-row 그리드에 스위치 두 행과 숫자 행 하나를 둔다.
//! 디스크 보관 줄 수는 디스크 스왑이 꺼져 있으면 비활성이고, -1 이 무제한이라 범위 하한이다.

use std::cell::RefCell;

use tasty_settings::{DEFAULT_SCROLLBACK_DISK_LINES, UNLIMITED_SCROLLBACK_DISK_LINES};
use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{Input, SettingsRow, settings_label_column};

use crate::catalog::spec::{self, StageVariant, TokenChip};
use crate::catalog::widgets::dialog as kit;

use super::settings_remote_transfer::WIDTH;

/// 본체 `tabs/performance.rs` 의 상한과 같은 값이다.
const DISK_LINES_MAX: i64 = 100_000_000;

struct Pair {
    polling: bool,
    disk_swap: bool,
    disk_lines: String,
}

thread_local! {
    // 짝 둘: 디스크 스왑 켬(기본 줄 수) · 디스크 스왑 끔(무제한 값, 비활성).
    static STATE: RefCell<[Pair; 2]> = RefCell::new([
        Pair {
            polling: true,
            disk_swap: true,
            disk_lines: DEFAULT_SCROLLBACK_DISK_LINES.to_string(),
        },
        Pair {
            polling: true,
            disk_swap: false,
            disk_lines: UNLIMITED_SCROLLBACK_DISK_LINES.to_string(),
        },
    ]);
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    let with_zoom =
        |base: Theme| Theme::with_colors_and_zoom(base.to_colors(), base.is_light, theme.ui_zoom);
    let themes = [
        (
            "Mocha · disk swap on",
            with_zoom(tasty_themes::mocha_fallback()),
        ),
        (
            "Latte · disk swap off",
            with_zoom(crate::host_shell::latte_theme()),
        ),
    ];
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        STATE.with(|s| {
            let state = &mut *s.borrow_mut();
            for (i, ((label, th), pair)) in themes.iter().zip(state.iter_mut()).enumerate() {
                spec::wrap_item(ui, |ui| {
                    ui.push_id(i, |ui| frame(ui, th, label, pair));
                });
            }
        });
    });

    spec::meta(
        ui,
        theme,
        &[
            (
                "rows",
                "polling switch · disk swap switch · lines kept on disk",
            ),
            (
                "field",
                "mono Input · field-width-xs 90 · right-aligned · no unit",
            ),
            ("range", "-1 (unlimited) … 100000000 · clamp on commit"),
            ("default value", "10000"),
            ("enabled when", "disk swap is on"),
        ],
        &[
            TokenChip::without_color("settings-row-gap", "→ space-md 12"),
            TokenChip::without_color("field-width-xs", "90"),
            TokenChip::new(
                "accent-danger",
                "out of range",
                theme.accent_danger().to_egui(),
            ),
        ],
    );
}

fn frame(ui: &mut egui::Ui, th: &Theme, label: &str, pair: &mut Pair) {
    use crate::i18n::t;
    kit::frame_card_flat(ui, th, WIDTH, kit::panel_fill(th), |ui| {
        kit::region_sym(ui, th.spacing_lg, th.spacing_md, |ui| {
            ui.spacing_mut().item_spacing.y = th.settings_row_gap().value();
            ui.label(
                egui::RichText::new(label)
                    .size(th.font_size_caption.value())
                    .color(th.text_muted().to_egui()),
            );
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
                tasty_ui_widgets::switch(ui, th, &mut pair.polling, None, true);
            });
            disk_swap.show(ui, th, col, |ui| {
                tasty_ui_widgets::switch(ui, th, &mut pair.disk_swap, None, true);
            });
            let enabled = pair.disk_swap;
            disk_lines.show(ui, th, col, |ui| {
                lines_control(ui, th, &mut pair.disk_lines, enabled)
            });
        });
    });
}

/// 숫자 Input, 범위 밖이면 그 아래 danger 한 줄(본체 `number_field` 와 같은 자리).
fn lines_control(ui: &mut egui::Ui, theme: &Theme, buf: &mut String, enabled: bool) {
    let (min, max) = (UNLIMITED_SCROLLBACK_DISK_LINES, DISK_LINES_MAX);
    let settled = buf
        .trim()
        .parse::<i64>()
        .ok()
        .map(|v| (v, v.clamp(min, max)))
        .and_then(|(v, c)| (v != c).then_some(c));
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = theme.spacing_xs.value();
        Input::new()
            .mono(true)
            .align(egui::Align::RIGHT)
            .width(theme.field_width_xs.value())
            .enabled(enabled)
            .invalid(settled.is_some())
            .show(ui, theme, buf);
        if let Some(settled) = settled {
            let line = crate::i18n::t("settings.number.range_between")
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
