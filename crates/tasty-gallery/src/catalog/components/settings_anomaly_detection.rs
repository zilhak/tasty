//! Received anomaly settings specimens; drafts here never persist to application settings.
use tasty_settings::*;
use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{AnomalySection, Input, SettingsRow, settings_anomaly};

use crate::catalog::spec::{self, StageVariant};
use crate::catalog::widgets::dialog as kit;

// 반응형 행을 비교하는 시안 액자의 전시 폭이다. 대응하는 Theme 프레임 역할은 없다.
const NARROW_FRAME_WIDTH: LogicalPx = LogicalPx(320.0);
const KOREAN_FRAME_WIDTH: LogicalPx = LogicalPx(440.0);

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Specimen {
    Default,
    AllOn,
    OffEditing,
    Invalid,
    Committed,
    Minimum,
    Maximum,
    Narrow,
}
pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    specimens(ui, theme, &[Specimen::Default]);
}
pub fn draw_off(ui: &mut egui::Ui, theme: &Theme) {
    specimens(ui, theme, &[Specimen::AllOn, Specimen::OffEditing]);
}
pub fn draw_range(ui: &mut egui::Ui, theme: &Theme) {
    specimens(
        ui,
        theme,
        &[
            Specimen::Invalid,
            Specimen::Committed,
            Specimen::Minimum,
            Specimen::Maximum,
        ],
    );
}
pub fn draw_narrow(ui: &mut egui::Ui, theme: &Theme) {
    specimens(ui, theme, &[Specimen::Narrow]);
}
fn specimens(ui: &mut egui::Ui, theme: &Theme, variants: &[Specimen]) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        for &variant in variants {
            let frames = if variant == Specimen::Narrow {
                vec![
                    (tasty_themes::mocha_fallback(), "en", NARROW_FRAME_WIDTH),
                    (crate::host_shell::latte_theme(), "ko", KOREAN_FRAME_WIDTH),
                    (tasty_themes::mocha_fallback(), "ja", NARROW_FRAME_WIDTH),
                ]
            } else {
                vec![
                    (
                        tasty_themes::mocha_fallback(),
                        "en",
                        super::settings_remote_transfer::WIDTH,
                    ),
                    (
                        crate::host_shell::latte_theme(),
                        "en",
                        super::settings_remote_transfer::WIDTH,
                    ),
                ]
            };
            for (i, (base, lang, width)) in frames.into_iter().enumerate() {
                let th =
                    Theme::with_colors_and_zoom(base.to_colors(), base.is_light, theme.ui_zoom);
                spec::wrap_item(ui, |ui| {
                    ui.push_id((i, variant), |ui| {
                        // Received frame widths and space-lg content insets.
                        kit::frame_card_flat(ui, &th, width, kit::panel_fill(&th), |ui| {
                            kit::region_sym(ui, th.spacing_lg, th.spacing_lg, |ui| {
                                let state_label = match variant {
                                    Specimen::Default => "default (true / false / true)",
                                    Specimen::AllOn => "all three on",
                                    Specimen::OffEditing => "Repeated calls off · editing 40",
                                    Specimen::Invalid => "Repeats of one call = 1 (typed)",
                                    Specimen::Committed => "after Enter / blur → committed 2",
                                    Specimen::Minimum => "every lower bound (valid)",
                                    Specimen::Maximum => "every upper bound (valid)",
                                    Specimen::Narrow => "narrow · ko / ja · responsive rows",
                                };
                                let theme_label = if th.is_light { "Latte" } else { "Mocha" };
                                ui.label(
                                    egui::RichText::new(format!("{theme_label} · {state_label}"))
                                        .size(th.font_size_caption.value())
                                        .color(th.text_muted().to_egui()),
                                );
                                ui.add_space(th.spacing_xs.value());
                                page(ui, &th, variant, lang);
                            });
                        });
                    });
                });
            }
        }
    });
    spec::note(
        ui,
        theme,
        "Notifications off hides notifications only. Detection, recording and editable values remain active. No restore button. Numeric fields commit on Enter or blur.",
    );
}
fn page(ui: &mut egui::Ui, th: &Theme, variant: Specimen, lang: &str) {
    let t = |key| crate::i18n::t_lang(lang, key);
    let ids = ["rss_surge", "slow_loop", "call_burst"];
    let fields = [
        "rss_surge_samples",
        "rss_surge_min_mib",
        "slow_loop_window_secs",
        "slow_loop_threshold",
        "call_burst_window_secs",
        "call_burst_threshold",
    ];
    let titles = [
        t("settings.anomaly.rss_surge.title"),
        t("settings.anomaly.slow_loop.title"),
        t("settings.anomaly.call_burst.title"),
    ];
    let captions = [
        t("settings.anomaly.rss_surge.caption"),
        t("settings.anomaly.slow_loop.caption"),
        t("settings.anomaly.call_burst.caption"),
    ];
    let labels = [
        t("settings.anomaly.rss_surge_samples.label"),
        t("settings.anomaly.rss_surge_min_mib.label"),
        t("settings.anomaly.slow_loop_window_secs.label"),
        t("settings.anomaly.slow_loop_threshold.label"),
        t("settings.anomaly.call_burst_window_secs.label"),
        t("settings.anomaly.call_burst_threshold.label"),
    ];
    let desc = [
        t("settings.anomaly.rss_surge_samples.caption"),
        t("settings.anomaly.rss_surge_min_mib.caption"),
        t("settings.anomaly.slow_loop_window_secs.caption"),
        t("settings.anomaly.slow_loop_threshold.caption"),
        t("settings.anomaly.call_burst_window_secs.caption"),
        t("settings.anomaly.call_burst_threshold.caption"),
    ];
    let sections = std::array::from_fn(|i| AnomalySection {
        title: titles[i],
        caption: captions[i],
        detector: ids[i],
        rows: [
            SettingsRow::new(t("settings.anomaly.show_notifications")),
            SettingsRow::new(labels[i * 2]).caption(desc[i * 2]),
            SettingsRow::new(labels[i * 2 + 1]).caption(desc[i * 2 + 1]),
        ],
    });
    let defaults = AnomalyDetectionSettings::default();
    let ranges = [
        RSS_SURGE_SAMPLES_RANGE,
        RSS_SURGE_MIN_MIB_RANGE,
        SLOW_LOOP_WINDOW_SECS_RANGE,
        SLOW_LOOP_THRESHOLD_RANGE,
        CALL_BURST_WINDOW_SECS_RANGE,
        CALL_BURST_THRESHOLD_RANGE,
    ];
    let mut values = [
        defaults.rss_surge_samples,
        defaults.rss_surge_min_mib,
        defaults.slow_loop_window_secs,
        defaults.slow_loop_threshold,
        defaults.call_burst_window_secs,
        defaults.call_burst_threshold,
    ];
    for i in 0..6 {
        if variant == Specimen::Minimum {
            values[i] = ranges[i].0;
        }
        if variant == Specimen::Maximum {
            values[i] = ranges[i].1;
        }
    }
    if variant == Specimen::Committed {
        values[3] = SLOW_LOOP_THRESHOLD_RANGE.0;
    }
    settings_anomaly(
        ui,
        th,
        t("settings.anomaly.notice"),
        t("settings.anomaly.detector"),
        &sections,
        |ui, section, row| {
            if row == 0 {
                let id = ui.id().with("notify");
                let mut value = ui.data_mut(|d| d.get_temp(id)).unwrap_or(
                    section != 1 || matches!(variant, Specimen::AllOn | Specimen::Maximum),
                );
                tasty_ui_widgets::switch(ui, th, &mut value, None, true);
                ui.data_mut(|d| d.insert_temp(id, value));
            } else {
                let i = section * 2 + row - 1;
                let id = ui.id().with(fields[i]);
                let (mut buf, mut committed) = ui
                    .data_mut(|d| d.get_temp::<(String, i128)>(id))
                    .unwrap_or_else(|| {
                        let value = if variant == Specimen::OffEditing && i == 3 {
                            40
                        } else {
                            i128::from(values[i])
                        };
                        (
                            match (variant, lang, i) {
                                (Specimen::Invalid, _, 3) | (Specimen::Narrow, "en", 3) => {
                                    "1".into()
                                }
                                (Specimen::Narrow, "ja", 5) => "5".into(),
                                _ => value.to_string(),
                            },
                            value,
                        )
                    });
                let pending = buf.trim().parse::<i128>().ok().and_then(|v| {
                    let n = v.clamp(ranges[i].0.into(), ranges[i].1.into());
                    (v != n).then_some(n)
                });
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = th.spacing_xs.value();
                    ui.horizontal(|ui| {
                        let width = tasty_ui_widgets::number_field_width(ui, th, &buf);
                        let response = Input::new()
                            .mono(true)
                            .align(egui::Align::RIGHT)
                            .width(width.value())
                            .invalid(pending.is_some())
                            .focus_look(
                                i == 3
                                    && matches!(variant, Specimen::Invalid | Specimen::OffEditing),
                            )
                            .show(ui, th, &mut buf);
                        tasty_ui_widgets::number_field_unit(
                            ui,
                            th,
                            [
                                t("settings.anomaly.rss_surge_samples.unit"),
                                t("settings.anomaly.rss_surge_min_mib.unit"),
                                t("settings.anomaly.slow_loop_window_secs.unit"),
                                t("settings.anomaly.slow_loop_threshold.unit"),
                                t("settings.anomaly.call_burst_window_secs.unit"),
                                t("settings.anomaly.call_burst_threshold.unit"),
                            ][i],
                            true,
                            true,
                        );
                        if response.lost_focus() {
                            if let Ok(v) = buf.trim().parse::<i128>() {
                                committed = v.clamp(ranges[i].0.into(), ranges[i].1.into());
                            }
                            buf = committed.to_string();
                        }
                    });
                    if let Some(n) = pending {
                        let note = tasty_i18n::fill_args(
                            t("settings.anomaly.range"),
                            &[
                                &ranges[i].0.to_string(),
                                &ranges[i].1.to_string(),
                                &n.to_string(),
                            ],
                        );
                        tasty_ui_widgets::number_field_note(ui, th, &note);
                    }
                });
                ui.data_mut(|d| d.insert_temp(id, (buf, committed)));
            }
        },
    );
}

#[cfg(test)]
mod tests;
