use std::collections::HashMap;

use crate::i18n::t;
use crate::settings::{
    EffectiveFont, FontOverride, FontSettings, GeneralSettings, HexColor, Settings, SurfaceColors,
};

/// Draw a label followed by a (?) icon with tooltip. For use inside Grid rows.
fn label_with_tooltip(ui: &mut egui::Ui, label: &str, tooltip: &str) {
    let th = crate::theme::theme();
    let text = egui::RichText::new(format!("{}  (?)", label));
    let response = ui.add(egui::Label::new(text).sense(egui::Sense::hover()));
    // Show tooltip only when hovering over the (?) portion
    if response.hovered() {
        response.show_tooltip_text(egui::RichText::new(tooltip).color(th.text));
    }
}

pub fn draw_general_tab(ui: &mut egui::Ui, settings: &mut Settings) {
    let th = crate::theme::theme();
    ui.add_space(8.0);

    if !settings.general.is_shell_valid() {
        ui.label(egui::RichText::new(t("settings.general.shell_not_found")).color(th.yellow));
        ui.add_space(4.0);
    }

    egui::Grid::new("general_grid")
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            ui.label(t("settings.general.shell_label"));
            if let Some(detected) = GeneralSettings::detect_bash() {
                if settings.general.shell.is_empty() || !settings.general.is_shell_valid() {
                    settings.general.shell = detected;
                }
            }
            ui.text_edit_singleline(&mut settings.general.shell);
            ui.end_row();

            ui.label(t("settings.general.shell_mode_label"));
            egui::ComboBox::from_id_salt("shell_mode")
                .selected_text(match settings.general.shell_mode.as_str() {
                    "tasty" => t("settings.general.shell_mode_tasty"),
                    "custom" => t("settings.general.shell_mode_custom"),
                    _ => t("settings.general.shell_mode_default"),
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut settings.general.shell_mode,
                        "default".to_string(),
                        t("settings.general.shell_mode_default"),
                    );
                    ui.selectable_value(
                        &mut settings.general.shell_mode,
                        "tasty".to_string(),
                        t("settings.general.shell_mode_tasty"),
                    );
                    ui.selectable_value(
                        &mut settings.general.shell_mode,
                        "custom".to_string(),
                        t("settings.general.shell_mode_custom"),
                    );
                });
            ui.end_row();

            if settings.general.shell_mode == "custom" {
                ui.label(t("settings.general.shell_args_label"));
                ui.text_edit_singleline(&mut settings.general.shell_args);
                ui.end_row();
            }

            ui.label(t("settings.general.startup_command_label"));
            ui.text_edit_singleline(&mut settings.general.startup_command);
            ui.end_row();

            ui.label(t("settings.general.scrollback_lines_label"));
            ui.add(
                egui::DragValue::new(&mut settings.general.scrollback_lines)
                    .range(0..=100000)
                    .speed(100),
            );
            ui.end_row();

            ui.label(t("settings.general.confirm_close_label"));
            ui.checkbox(&mut settings.general.confirm_close_running, "");
            ui.end_row();

            ui.label(t("settings.general.inherit_cwd_label"));
            ui.checkbox(&mut settings.general.inherit_cwd, "");
            ui.end_row();

            ui.label(t("settings.general.restore_layout_label"));
            ui.checkbox(&mut settings.general.restore_layout, "");
            ui.end_row();

            ui.label(t("settings.general.close_behavior_label"));
            egui::ComboBox::from_id_salt("close_behavior")
                .selected_text(match settings.general.close_behavior.as_str() {
                    "quit" => t("settings.general.close_behavior_quit"),
                    "minimize" => t("settings.general.close_behavior_minimize"),
                    _ => t("settings.general.close_behavior_ask"),
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut settings.general.close_behavior,
                        "ask".to_string(),
                        t("settings.general.close_behavior_ask"),
                    );
                    ui.selectable_value(
                        &mut settings.general.close_behavior,
                        "minimize".to_string(),
                        t("settings.general.close_behavior_minimize"),
                    );
                    ui.selectable_value(
                        &mut settings.general.close_behavior,
                        "quit".to_string(),
                        t("settings.general.close_behavior_quit"),
                    );
                });
            ui.end_row();

            ui.label(t("settings.general.link_modifier_label"));
            egui::ComboBox::from_id_salt("link_modifier")
                .selected_text(match settings.general.link_click_modifier.as_str() {
                    "alt" => t("settings.general.link_modifier_alt"),
                    "none" => t("settings.general.link_modifier_none"),
                    _ => t("settings.general.link_modifier_ctrl"),
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut settings.general.link_click_modifier,
                        "ctrl".to_string(),
                        t("settings.general.link_modifier_ctrl"),
                    );
                    ui.selectable_value(
                        &mut settings.general.link_click_modifier,
                        "alt".to_string(),
                        t("settings.general.link_modifier_alt"),
                    );
                    ui.selectable_value(
                        &mut settings.general.link_click_modifier,
                        "none".to_string(),
                        t("settings.general.link_modifier_none"),
                    );
                });
            ui.end_row();
        });
}

pub fn draw_appearance_tab(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    sub_tab: &mut super::AppearanceSubTab,
    font_families: &mut Option<Vec<String>>,
    font_filter: &mut HashMap<String, String>,
    preview_font_loaded: &mut HashMap<String, String>,
) {
    use super::AppearanceSubTab;
    let th = crate::theme::theme();
    ui.add_space(8.0);

    let available_height = ui.available_height() - 8.0 - 14.0;

    ui.horizontal_top(|ui| {
        // ── Left: sub-tab selector ──
        egui::Frame::new()
            .fill(th.crust)
            .stroke(egui::Stroke::new(1.0, th.surface0))
            .corner_radius(4.0)
            .inner_margin(egui::Margin::symmetric(6, 6))
            .show(ui, |ui| {
                ui.set_width(100.0);
                ui.set_min_height(available_height);

                ui.vertical(|ui| {
                    let sub_tabs = [
                        (
                            AppearanceSubTab::Theme,
                            t("settings.appearance.subtab.theme"),
                        ),
                        (
                            AppearanceSubTab::General,
                            t("settings.appearance.subtab.general"),
                        ),
                        (AppearanceSubTab::Tasty, "Tasty"),
                        (
                            AppearanceSubTab::Terminal,
                            t("settings.appearance.subtab.terminal"),
                        ),
                        (
                            AppearanceSubTab::Markdown,
                            t("settings.appearance.subtab.markdown"),
                        ),
                        (AppearanceSubTab::Explorer, "Explorer"),
                        (AppearanceSubTab::HtmlViewer, "HTML"),
                    ];
                    for (tab, label) in &sub_tabs {
                        let selected = *sub_tab == *tab;
                        if ui.selectable_label(selected, *label).clicked() {
                            *sub_tab = *tab;
                        }
                    }
                });
            });

        ui.add_space(8.0);

        // ── Right: sub-tab content ──
        ui.vertical(|ui| match *sub_tab {
            AppearanceSubTab::Theme => {
                draw_appearance_theme(ui, settings, font_families, font_filter, preview_font_loaded);
            }
            AppearanceSubTab::General => {
                draw_appearance_general(ui, settings);
            }
            AppearanceSubTab::Tasty => {
                draw_appearance_tasty(ui, settings);
            }
            AppearanceSubTab::Terminal => {
                draw_appearance_terminal(
                    ui,
                    settings,
                    font_families,
                    font_filter,
                    preview_font_loaded,
                );
            }
            AppearanceSubTab::Markdown => {
                draw_appearance_markdown(
                    ui,
                    settings,
                    font_families,
                    font_filter,
                    preview_font_loaded,
                );
            }
            AppearanceSubTab::Explorer => {
                draw_appearance_explorer(
                    ui,
                    settings,
                    font_families,
                    font_filter,
                    preview_font_loaded,
                );
            }
            AppearanceSubTab::HtmlViewer => {
                draw_appearance_placeholder(ui, "HTML Viewer");
            }
        });
    });
}

/// Appearance > Theme: preset selection + default font settings (single source
/// of truth for fields not overridden per-surface).
fn draw_appearance_theme(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    font_families: &mut Option<Vec<String>>,
    font_filter: &mut HashMap<String, String>,
    preview_font_loaded: &mut HashMap<String, String>,
) {
    let th = crate::theme::theme();
    ui.add_space(8.0);

    ui.label(
        egui::RichText::new(t("settings.appearance.theme.heading"))
            .strong()
            .color(th.text),
    );
    ui.add_space(8.0);

    let presets = crate::theme::presets();
    for preset in &presets {
        let is_current = settings.appearance.theme == preset.id;
        let response = ui.selectable_label(is_current, preset.label);
        if response.clicked() && !is_current {
            settings.appearance.theme = preset.id.to_string();
            settings.appearance.terminal_colors = preset.terminal_colors.clone();
            settings.appearance.markdown_colors = preset.markdown_colors.clone();
            settings.appearance.explorer_colors = preset.explorer_colors.clone();
        }
    }

    ui.add_space(8.0);
    ui.label(
        egui::RichText::new(t("settings.appearance.theme.hint"))
            .small()
            .color(th.subtext0),
    );

    ui.add_space(16.0);
    ui.separator();
    ui.add_space(8.0);

    ui.label(
        egui::RichText::new(t("settings.appearance.font.default_heading"))
            .strong()
            .color(th.text),
    );
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new(t("settings.appearance.font.default_hint"))
            .small()
            .color(th.subtext0),
    );
    ui.add_space(8.0);

    ui.columns(2, |columns| {
        font_settings_grid(
            &mut columns[0],
            &mut settings.appearance.default_font,
            font_families,
            font_filter,
            "default",
        );
        let preview_eff = effective_from_settings(&settings.appearance.default_font);
        draw_font_preview(
            &mut columns[1],
            &preview_eff,
            &settings.appearance.terminal_colors,
            &settings.appearance,
            "default",
            preview_font_loaded,
        );
    });
}

/// Convert FontSettings → EffectiveFont (used for default-font preview).
fn effective_from_settings(s: &FontSettings) -> EffectiveFont {
    EffectiveFont {
        font_family: s.font_family.clone(),
        font_size: s.font_size,
        custom_font_path: s.custom_font_path.clone(),
        line_height: s.line_height,
        font_scale_mode: s.font_scale_mode.clone(),
    }
}

/// Appearance > General: theme, background opacity, focused surface bg
fn draw_appearance_general(ui: &mut egui::Ui, settings: &mut Settings) {
    egui::Grid::new("appearance_general_grid")
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            ui.label(t("settings.appearance.background_opacity_label"));
            ui.add(egui::Slider::new(
                &mut settings.appearance.background_opacity,
                0.0..=1.0,
            ));
            ui.end_row();
        });
}

/// Appearance > Tasty: UI scale, sidebar width
fn draw_appearance_tasty(ui: &mut egui::Ui, settings: &mut Settings) {
    egui::Grid::new("appearance_tasty_grid")
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            ui.label(t("settings.appearance.ui_scale_label"));
            egui::ComboBox::from_id_salt("ui_scale")
                .selected_text(match settings.appearance.ui_scale.as_str() {
                    "small" => t("settings.appearance.ui_scale_small"),
                    "large" => t("settings.appearance.ui_scale_large"),
                    _ => t("settings.appearance.ui_scale_medium"),
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut settings.appearance.ui_scale,
                        "small".to_string(),
                        t("settings.appearance.ui_scale_small"),
                    );
                    ui.selectable_value(
                        &mut settings.appearance.ui_scale,
                        "medium".to_string(),
                        t("settings.appearance.ui_scale_medium"),
                    );
                    ui.selectable_value(
                        &mut settings.appearance.ui_scale,
                        "large".to_string(),
                        t("settings.appearance.ui_scale_large"),
                    );
                });
            ui.end_row();

            ui.label(t("settings.appearance.sidebar_width_label"));
            ui.add(
                egui::DragValue::new(&mut settings.appearance.sidebar_width.0)
                    .range(100.0..=400.0)
                    .speed(1.0),
            );
            ui.end_row();
        });
}

/// Appearance > Terminal: font override + preview + colors
fn draw_appearance_terminal(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    font_families: &mut Option<Vec<String>>,
    font_filter: &mut HashMap<String, String>,
    preview_font_loaded: &mut HashMap<String, String>,
) {
    draw_surface_font_section(
        ui,
        settings,
        font_families,
        font_filter,
        preview_font_loaded,
        SurfaceFontTarget::Terminal,
    );

    ui.add_space(16.0);
    ui.separator();
    ui.add_space(8.0);

    draw_surface_colors(ui, "terminal", &mut settings.appearance.terminal_colors);
}

#[derive(Clone, Copy)]
enum SurfaceFontTarget {
    Terminal,
    Markdown,
    Explorer,
}

impl SurfaceFontTarget {
    fn salt(self) -> &'static str {
        match self {
            SurfaceFontTarget::Terminal => "terminal",
            SurfaceFontTarget::Markdown => "markdown",
            SurfaceFontTarget::Explorer => "explorer",
        }
    }

    fn override_mut(self, app: &mut crate::settings::AppearanceSettings) -> &mut FontOverride {
        match self {
            SurfaceFontTarget::Terminal => &mut app.terminal_font,
            SurfaceFontTarget::Markdown => &mut app.markdown_font,
            SurfaceFontTarget::Explorer => &mut app.explorer_font,
        }
    }

    fn effective(self, app: &crate::settings::AppearanceSettings) -> EffectiveFont {
        match self {
            SurfaceFontTarget::Terminal => app.effective_terminal_font(),
            SurfaceFontTarget::Markdown => app.effective_markdown_font(),
            SurfaceFontTarget::Explorer => app.effective_explorer_font(),
        }
    }

    fn colors<'a>(
        self,
        app: &'a crate::settings::AppearanceSettings,
    ) -> &'a SurfaceColors {
        match self {
            SurfaceFontTarget::Terminal => &app.terminal_colors,
            SurfaceFontTarget::Markdown => &app.markdown_colors,
            SurfaceFontTarget::Explorer => &app.explorer_colors,
        }
    }
}

fn draw_surface_font_section(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    font_families: &mut Option<Vec<String>>,
    font_filter: &mut HashMap<String, String>,
    preview_font_loaded: &mut HashMap<String, String>,
    target: SurfaceFontTarget,
) {
    let th = crate::theme::theme();
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new(t("settings.appearance.font.override_heading"))
            .strong()
            .color(th.text),
    );
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new(t("settings.appearance.font.override_hint"))
            .small()
            .color(th.subtext0),
    );
    ui.add_space(8.0);

    ui.columns(2, |columns| {
        let default_font = settings.appearance.default_font.clone();
        font_override_grid(
            &mut columns[0],
            target.override_mut(&mut settings.appearance),
            &default_font,
            font_families,
            font_filter,
            target.salt(),
        );

        let eff = target.effective(&settings.appearance);
        let colors = target.colors(&settings.appearance).clone();
        draw_font_preview(
            &mut columns[1],
            &eff,
            &colors,
            &settings.appearance,
            target.salt(),
            preview_font_loaded,
        );
    });
}

/// Draw a single color row: label + color picker button + hex text input.
fn draw_color_row(ui: &mut egui::Ui, label: &str, color: &mut HexColor) {
    ui.label(label);
    ui.horizontal(|ui| {
        egui::widgets::color_picker::color_edit_button_srgba(
            ui,
            &mut color.0,
            egui::color_picker::Alpha::Opaque,
        );
        let mut hex = color.to_hex();
        let response = ui.add(egui::TextEdit::singleline(&mut hex).desired_width(80.0));
        if response.changed() {
            if let Some(parsed) = HexColor::from_hex(&hex) {
                *color = parsed;
            }
        }
    });
    ui.end_row();
}

/// Draw color settings for a SurfaceColors (focused/unfocused bg/fg).
fn draw_surface_colors(ui: &mut egui::Ui, id_salt: &str, colors: &mut crate::settings::SurfaceColors) {
    let th = crate::theme::theme();

    ui.label(
        egui::RichText::new(t("settings.appearance.colors.focused_heading"))
            .strong()
            .color(th.text),
    );
    ui.end_row();

    egui::Grid::new(format!("{}_focused_grid", id_salt))
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            draw_color_row(ui, t("settings.appearance.colors.bg_label"), &mut colors.focused_bg);
            draw_color_row(ui, t("settings.appearance.colors.fg_label"), &mut colors.focused_fg);
        });

    ui.add_space(12.0);

    ui.label(
        egui::RichText::new(t("settings.appearance.colors.unfocused_heading"))
            .strong()
            .color(th.text),
    );
    ui.end_row();

    egui::Grid::new(format!("{}_unfocused_grid", id_salt))
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            draw_color_row(ui, t("settings.appearance.colors.bg_label"), &mut colors.unfocused_bg);
            draw_color_row(ui, t("settings.appearance.colors.fg_label"), &mut colors.unfocused_fg);
        });
}

/// Appearance > Markdown: font override + color settings.
fn draw_appearance_markdown(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    font_families: &mut Option<Vec<String>>,
    font_filter: &mut HashMap<String, String>,
    preview_font_loaded: &mut HashMap<String, String>,
) {
    draw_surface_font_section(
        ui,
        settings,
        font_families,
        font_filter,
        preview_font_loaded,
        SurfaceFontTarget::Markdown,
    );

    ui.add_space(16.0);
    ui.separator();
    ui.add_space(8.0);

    draw_surface_colors(ui, "markdown", &mut settings.appearance.markdown_colors);
}

/// Appearance > Explorer: font override + color settings.
fn draw_appearance_explorer(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    font_families: &mut Option<Vec<String>>,
    font_filter: &mut HashMap<String, String>,
    preview_font_loaded: &mut HashMap<String, String>,
) {
    draw_surface_font_section(
        ui,
        settings,
        font_families,
        font_filter,
        preview_font_loaded,
        SurfaceFontTarget::Explorer,
    );

    ui.add_space(16.0);
    ui.separator();
    ui.add_space(8.0);

    draw_surface_colors(ui, "explorer", &mut settings.appearance.explorer_colors);
}

/// Placeholder for sub-tabs not yet populated with settings.
fn draw_appearance_placeholder(ui: &mut egui::Ui, name: &str) {
    let th = crate::theme::theme();
    ui.add_space(20.0);
    ui.label(
        egui::RichText::new(format!("{} appearance settings (coming soon)", name))
            .color(th.subtext0),
    );
}

/// Searchable font family combo. `value` is the family name in the underlying
/// data (`""` means "monospace default"). `salt` uniquifies the combo id and
/// the per-combo filter cache key.
fn font_family_picker(
    ui: &mut egui::Ui,
    value: &mut String,
    font_families: &Option<Vec<String>>,
    font_filter: &mut HashMap<String, String>,
    salt: &str,
    enabled: bool,
) {
    let th = crate::theme::theme();
    let display_name = if value.is_empty() {
        "monospace (default)".to_string()
    } else {
        value.clone()
    };
    let combo_id = format!("font_family_combo_{}", salt);
    let filter = font_filter.entry(salt.to_string()).or_default();

    ui.add_enabled_ui(enabled, |ui| {
        egui::ComboBox::from_id_salt(combo_id)
            .selected_text(&display_name)
            .width(200.0)
            .height(300.0)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show_ui(ui, |ui| {
                ui.add(
                    egui::TextEdit::singleline(filter)
                        .hint_text(t("settings.appearance.search_hint"))
                        .desired_width(190.0),
                );
                ui.separator();

                let filter_lower = filter.to_lowercase();
                if filter_lower.is_empty() || "monospace".contains(&filter_lower) {
                    if ui
                        .selectable_label(value.is_empty(), "monospace (default)")
                        .clicked()
                    {
                        value.clear();
                    }
                }

                if let Some(families) = font_families {
                    egui::ScrollArea::vertical()
                        .max_height(250.0)
                        .drag_to_scroll(false)
                        .show(ui, |ui| {
                            for family in families {
                                if !filter_lower.is_empty()
                                    && !family.to_lowercase().contains(&filter_lower)
                                {
                                    continue;
                                }
                                let selected = value == family;
                                if ui.selectable_label(selected, family).clicked() {
                                    *value = family.clone();
                                }
                            }
                        });
                } else {
                    ui.label(
                        egui::RichText::new(t("settings.appearance.loading_fonts"))
                            .color(th.subtext0),
                    );
                }
            });
    });
}

/// Edit a `FontSettings` (no fallback semantics — every field is always set).
fn font_settings_grid(
    ui: &mut egui::Ui,
    font: &mut FontSettings,
    font_families: &mut Option<Vec<String>>,
    font_filter: &mut HashMap<String, String>,
    salt: &str,
) {
    egui::Grid::new(format!("font_settings_grid_{}", salt))
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            ui.label(t("settings.appearance.font_family_label"));
            font_family_picker(ui, &mut font.font_family, font_families, font_filter, salt, true);
            ui.end_row();

            ui.label(t("settings.appearance.custom_font_label"));
            ui.text_edit_singleline(&mut font.custom_font_path);
            ui.end_row();

            ui.label(t("settings.appearance.font_size_label"));
            ui.add(
                egui::DragValue::new(&mut font.font_size)
                    .range(6.0..=72.0)
                    .speed(0.5),
            );
            ui.end_row();

            label_with_tooltip(
                ui,
                t("settings.appearance.line_height_label"),
                t("settings.appearance.line_height_tooltip"),
            );
            ui.add(
                egui::DragValue::new(&mut font.line_height)
                    .range(0.8..=2.0)
                    .speed(0.05)
                    .max_decimals(2),
            );
            ui.end_row();

            label_with_tooltip(
                ui,
                t("settings.appearance.font_scale_mode_label"),
                t("settings.appearance.font_scale_mode_tooltip"),
            );
            font_scale_mode_combo(ui, &mut font.font_scale_mode, salt, true);
            ui.end_row();
        });
}

/// Edit a `FontOverride` against a `FontSettings` default. Each row has a
/// "use default" checkbox: checked → override field is `None` (input
/// disabled, default value shown for reference). Unchecked → override is
/// `Some(current_effective_value)` and the input is enabled.
fn font_override_grid(
    ui: &mut egui::Ui,
    ov: &mut FontOverride,
    default: &FontSettings,
    font_families: &mut Option<Vec<String>>,
    font_filter: &mut HashMap<String, String>,
    salt: &str,
) {
    egui::Grid::new(format!("font_override_grid_{}", salt))
        .num_columns(3)
        .spacing([8.0, 8.0])
        .show(ui, |ui| {
            // ── Font family ──
            ui.label(t("settings.appearance.font_family_label"));
            override_checkbox(ui, &mut ov.font_family, || default.font_family.clone(), salt);
            let mut family_value = ov
                .font_family
                .clone()
                .unwrap_or_else(|| default.font_family.clone());
            font_family_picker(
                ui,
                &mut family_value,
                font_families,
                font_filter,
                salt,
                ov.font_family.is_some(),
            );
            if let Some(stored) = ov.font_family.as_mut() {
                *stored = family_value;
            }
            ui.end_row();

            // ── Custom font path ──
            ui.label(t("settings.appearance.custom_font_label"));
            override_checkbox(
                ui,
                &mut ov.custom_font_path,
                || default.custom_font_path.clone(),
                salt,
            );
            let mut path_value = ov
                .custom_font_path
                .clone()
                .unwrap_or_else(|| default.custom_font_path.clone());
            ui.add_enabled_ui(ov.custom_font_path.is_some(), |ui| {
                ui.text_edit_singleline(&mut path_value);
            });
            if let Some(stored) = ov.custom_font_path.as_mut() {
                *stored = path_value;
            }
            ui.end_row();

            // ── Font size ──
            ui.label(t("settings.appearance.font_size_label"));
            override_checkbox(ui, &mut ov.font_size, || default.font_size, salt);
            let mut size_value = ov.font_size.unwrap_or(default.font_size);
            ui.add_enabled_ui(ov.font_size.is_some(), |ui| {
                ui.add(
                    egui::DragValue::new(&mut size_value)
                        .range(6.0..=72.0)
                        .speed(0.5),
                );
            });
            if let Some(stored) = ov.font_size.as_mut() {
                *stored = size_value;
            }
            ui.end_row();

            // ── Line height ──
            label_with_tooltip(
                ui,
                t("settings.appearance.line_height_label"),
                t("settings.appearance.line_height_tooltip"),
            );
            override_checkbox(ui, &mut ov.line_height, || default.line_height, salt);
            let mut lh_value = ov.line_height.unwrap_or(default.line_height);
            ui.add_enabled_ui(ov.line_height.is_some(), |ui| {
                ui.add(
                    egui::DragValue::new(&mut lh_value)
                        .range(0.8..=2.0)
                        .speed(0.05)
                        .max_decimals(2),
                );
            });
            if let Some(stored) = ov.line_height.as_mut() {
                *stored = lh_value;
            }
            ui.end_row();

            // ── Font scale mode ──
            label_with_tooltip(
                ui,
                t("settings.appearance.font_scale_mode_label"),
                t("settings.appearance.font_scale_mode_tooltip"),
            );
            override_checkbox(
                ui,
                &mut ov.font_scale_mode,
                || default.font_scale_mode.clone(),
                salt,
            );
            let mut mode_value = ov
                .font_scale_mode
                .clone()
                .unwrap_or_else(|| default.font_scale_mode.clone());
            font_scale_mode_combo(ui, &mut mode_value, salt, ov.font_scale_mode.is_some());
            if let Some(stored) = ov.font_scale_mode.as_mut() {
                *stored = mode_value;
            }
            ui.end_row();
        });
}

/// "Use default" checkbox: checked when override is None.
/// Toggling on → set to None; toggling off → seed with the current default.
fn override_checkbox<T, F>(ui: &mut egui::Ui, slot: &mut Option<T>, default_provider: F, _salt: &str)
where
    F: FnOnce() -> T,
{
    let mut use_default = slot.is_none();
    let label = t("settings.appearance.font.use_default_label");
    if ui.checkbox(&mut use_default, label).changed() {
        if use_default {
            *slot = None;
        } else if slot.is_none() {
            *slot = Some(default_provider());
        }
    }
}

fn font_scale_mode_combo(ui: &mut egui::Ui, value: &mut String, salt: &str, enabled: bool) {
    ui.add_enabled_ui(enabled, |ui| {
        let combo_id = format!("font_scale_mode_{}", salt);
        egui::ComboBox::from_id_salt(combo_id)
            .selected_text(match value.as_str() {
                "auto" => t("settings.appearance.font_scale_mode_auto"),
                _ => t("settings.appearance.font_scale_mode_fixed"),
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    value,
                    "auto".to_string(),
                    t("settings.appearance.font_scale_mode_auto"),
                );
                ui.selectable_value(
                    value,
                    "fixed".to_string(),
                    t("settings.appearance.font_scale_mode_fixed"),
                );
            });
    });
}

/// Draw a 2-row colored preview block for an `EffectiveFont`. `slot` is a
/// short id ("default"/"terminal"/"markdown"/"explorer") used as both the egui
/// font family slot name and the cache key in `preview_font_loaded`.
fn draw_font_preview(
    ui: &mut egui::Ui,
    eff: &EffectiveFont,
    colors: &SurfaceColors,
    appearance: &crate::settings::AppearanceSettings,
    slot: &str,
    preview_font_loaded: &mut HashMap<String, String>,
) {
    let th = crate::theme::theme();
    ui.heading(t("settings.appearance.preview_heading"));
    ui.add_space(4.0);

    let slot_name = format!("preview_{}", slot);
    let display_family = if eff.font_family.is_empty() {
        "monospace".to_string()
    } else {
        eff.font_family.clone()
    };

    // Decide which egui FontFamily to render text in. We try to load the
    // requested family into a per-slot named family the first time we see it,
    // and remember success/failure in `preview_font_loaded` so we don't retry.
    let preview_family = if eff.font_family.is_empty() && eff.custom_font_path.is_empty() {
        egui::FontFamily::Monospace
    } else {
        let key = format!("{}|{}", eff.font_family, eff.custom_font_path);
        let failed_marker = format!("\x00:{}", key);
        let cached = preview_font_loaded.get(&slot_name).cloned().unwrap_or_default();
        if cached == key {
            egui::FontFamily::Name(slot_name.clone().into())
        } else if cached == failed_marker {
            egui::FontFamily::Monospace
        } else {
            // First attempt this frame: rebuild the full FontDefinitions
            // (surface families + this preview slot) and install it.
            let fonts =
                crate::ui::font_registry::build_font_definitions(appearance, Some((&slot_name, eff)));
            ui.ctx().set_fonts(fonts);
            // set_fonts() replaces the entire FontDefinitions, so other
            // preview slots are no longer registered. Clear them so they
            // get re-loaded when their tab is revisited.
            preview_font_loaded.retain(|k, _| k == &slot_name);
            preview_font_loaded.insert(slot_name.clone(), key);
            // The family won't be available until the next frame; fall back
            // to Monospace this frame.
            egui::FontFamily::Monospace
        }
    };

    let sample_lines = [
        "AaBbCcDdEeFfGg",
        "\u{AC00}\u{B098}\u{B2E4}\u{B77C}\u{B9C8}\u{BC14}\u{C0AC}", // 가나다라마바사
        "1234567890",
        "\u{30A2}\u{30AB}\u{30B5}\u{30BF}\u{30CA}\u{30CF}\u{30DE}\u{30E9}\u{30E4}\u{30EF}", // アカサタナハマラヤワ
    ];

    let focused_bg32 = colors.focused_bg.0;
    let unfocused_bg32 = colors.unfocused_bg.0;
    let fg32 = colors.focused_fg.0;

    let font_size = eff.font_size.max(1.0);
    let preview_font = egui::FontId::new(font_size, preview_family);
    let line_height = font_size * 1.4;
    let padding = 8.0;
    let block_height = line_height * sample_lines.len() as f32 + padding * 2.0;

    // ── Focused preview ──
    ui.label(
        egui::RichText::new(t("settings.appearance.preview_focused"))
            .size(th.font_size_caption.value())
            .color(th.subtext0),
    );
    let (focused_rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), block_height),
        egui::Sense::hover(),
    );
    ui.painter().rect_filled(focused_rect, 2.0, focused_bg32);
    ui.painter().rect_stroke(
        focused_rect,
        2.0,
        egui::Stroke::new(th.border_width.value(), th.blue),
        egui::StrokeKind::Outside,
    );
    for (i, line) in sample_lines.iter().enumerate() {
        let pos = focused_rect.min + egui::vec2(padding, padding + line_height * i as f32);
        ui.painter().text(
            pos,
            egui::Align2::LEFT_TOP,
            line,
            preview_font.clone(),
            fg32,
        );
    }

    ui.add_space(8.0);

    // ── Unfocused preview ──
    ui.label(
        egui::RichText::new(t("settings.appearance.preview_unfocused"))
            .size(th.font_size_caption.value())
            .color(th.subtext0),
    );
    let (unfocused_rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), block_height),
        egui::Sense::hover(),
    );
    ui.painter()
        .rect_filled(unfocused_rect, 2.0, unfocused_bg32);
    for (i, line) in sample_lines.iter().enumerate() {
        let pos = unfocused_rect.min + egui::vec2(padding, padding + line_height * i as f32);
        ui.painter().text(
            pos,
            egui::Align2::LEFT_TOP,
            line,
            preview_font.clone(),
            fg32,
        );
    }

    ui.add_space(8.0);
    ui.label(
        egui::RichText::new(crate::i18n::t_fmt(
            "settings.appearance.preview_font_info",
            &format!("{} / {:.1}px", display_family, font_size),
        ))
        .size(th.font_size_caption.value())
        .color(th.subtext0),
    );
}

/// Clipboard 탭: 히스토리 기능 설정.
/// (복사/붙여넣기/줌 단축키 설정은 Keybindings 탭의 Clipboard/Zoom 서브탭으로 이관됨.)
pub fn draw_clipboard_tab(ui: &mut egui::Ui, settings: &mut Settings) {
    let th = crate::theme::theme();
    ui.add_space(8.0);

    ui.heading(t("settings.clipboard.history_heading"));
    ui.add_space(4.0);

    ui.checkbox(
        &mut settings.clipboard.history_enabled,
        t("settings.clipboard.history_enabled_label"),
    );
    egui::Grid::new("clipboard_history_grid")
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            ui.label(t("settings.clipboard.history_max_label"));
            ui.add(
                egui::DragValue::new(&mut settings.clipboard.history_max)
                    .range(1..=1000)
                    .speed(1),
            );
            ui.end_row();

            ui.label(t("settings.clipboard.poll_interval_ms_label"));
            ui.add(
                egui::DragValue::new(&mut settings.clipboard.poll_interval_ms)
                    .range(100..=10000)
                    .speed(50),
            );
            ui.end_row();
        });
    ui.label(
        egui::RichText::new(t("settings.clipboard.poll_interval_restart_notice"))
            .small()
            .color(th.yellow),
    );
}

pub fn draw_notifications_tab(ui: &mut egui::Ui, settings: &mut Settings) {
    ui.add_space(8.0);

    ui.checkbox(
        &mut settings.notification.enabled,
        t("settings.notifications.enabled"),
    );
    ui.checkbox(
        &mut settings.notification.system_notification,
        t("settings.notifications.system_notification"),
    );
    ui.checkbox(
        &mut settings.notification.sound,
        t("settings.notifications.sound"),
    );

    ui.add_space(8.0);
    egui::Grid::new("notification_grid")
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            ui.label(t("settings.notifications.coalesce_interval_label"));
            ui.add(
                egui::DragValue::new(&mut settings.notification.coalesce_ms)
                    .range(0..=5000)
                    .speed(50),
            );
            ui.end_row();
        });
}

pub fn draw_language_tab(ui: &mut egui::Ui, settings: &mut Settings) {
    let th = crate::theme::theme();
    ui.add_space(8.0);

    egui::Grid::new("language_grid")
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            ui.label(t("settings.language.label"));
            egui::ComboBox::from_id_salt("language_select")
                .selected_text(language_display_name(&settings.general.language))
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut settings.general.language,
                        "en".to_string(),
                        "English",
                    );
                    ui.selectable_value(&mut settings.general.language, "ko".to_string(), "한국어");
                    ui.selectable_value(&mut settings.general.language, "ja".to_string(), "日本語");
                });
            ui.end_row();
        });

    ui.add_space(8.0);
    ui.label(
        egui::RichText::new(t("settings.language.restart_notice"))
            .small()
            .color(th.yellow),
    );
}

fn language_display_name(code: &str) -> &str {
    match code {
        "en" => "English",
        "ko" => "한국어",
        "ja" => "日本語",
        _ => code,
    }
}

/// Misc / 기타 탭: 좌측 서브탭 메뉴 + 우측 콘텐츠.
pub fn draw_misc_tab(
    ui: &mut egui::Ui,
    sub_tab: &mut crate::settings_ui::MiscSubTab,
    bashrc_user_draft: &mut Option<String>,
) {
    let th = crate::theme::theme();
    ui.add_space(8.0);

    let available_height = ui.available_height() - 8.0 - 14.0;

    ui.horizontal_top(|ui| {
        egui::Frame::new()
            .fill(th.crust)
            .stroke(egui::Stroke::new(1.0, th.surface0))
            .corner_radius(4.0)
            .inner_margin(egui::Margin::symmetric(6, 6))
            .show(ui, |ui| {
                ui.set_width(100.0);
                ui.set_min_height(available_height);

                ui.vertical(|ui| {
                    let sub_tabs = [(
                        crate::settings_ui::MiscSubTab::Tastyrc,
                        t("settings.misc.subtab.tastyrc"),
                    )];

                    for (tab, label) in &sub_tabs {
                        let selected = *sub_tab == *tab;
                        if ui.selectable_label(selected, *label).clicked() {
                            *sub_tab = *tab;
                        }
                    }
                });
            });

        ui.add_space(8.0);

        ui.vertical(|ui| match *sub_tab {
            crate::settings_ui::MiscSubTab::Tastyrc => draw_tastyrc_subtab(ui, bashrc_user_draft),
        });
    });
}

/// tastyrc 서브탭: Tasty 모드에서 적용되는 bashrc 사용자 영역 편집.
fn draw_tastyrc_subtab(ui: &mut egui::Ui, bashrc_user_draft: &mut Option<String>) {
    let th = crate::theme::theme();

    ui.heading(t("settings.misc.bashrc.heading"));
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new(t("settings.misc.bashrc.description"))
            .small()
            .color(th.subtext0),
    );
    ui.add_space(8.0);

    // draft는 mod.rs 진입부에서 lazy 로드되므로 이 시점엔 항상 Some.
    let draft = bashrc_user_draft.get_or_insert_with(crate::settings::general::load_user_bashrc);

    ui.horizontal(|ui| {
        if ui.button(t("settings.misc.bashrc.reset_button")).clicked() {
            *draft = crate::settings::general::INITIAL_USER_BASHRC.to_string();
        }
    });
    ui.add_space(4.0);

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .drag_to_scroll(false)
        .show(ui, |ui| {
            ui.add(
                egui::TextEdit::multiline(draft)
                    .font(egui::TextStyle::Monospace)
                    .desired_rows(20)
                    .desired_width(f32::INFINITY)
                    .code_editor(),
            );
        });
}

pub fn draw_performance_tab(ui: &mut egui::Ui, settings: &mut Settings) {
    let th = crate::theme::theme();
    ui.add_space(8.0);
    ui.label(
        egui::RichText::new(t("settings.performance.restart_notice"))
            .small()
            .color(th.yellow),
    );
    ui.add_space(12.0);

    ui.checkbox(
        &mut settings.performance.targeted_pty_polling,
        t("settings.performance.targeted_pty_polling"),
    );
    ui.label(
        egui::RichText::new(t("settings.performance.targeted_pty_polling_desc"))
            .small()
            .color(egui::Color32::GRAY),
    );
    ui.add_space(8.0);

    ui.checkbox(
        &mut settings.performance.scrollback_disk_swap,
        t("settings.performance.scrollback_disk_swap"),
    );
    ui.label(
        egui::RichText::new(t("settings.performance.scrollback_disk_swap_desc"))
            .small()
            .color(egui::Color32::GRAY),
    );
    ui.add_space(8.0);

    ui.checkbox(
        &mut settings.performance.lazy_pty_init,
        t("settings.performance.lazy_pty_init"),
    );
    ui.label(
        egui::RichText::new(t("settings.performance.lazy_pty_init_desc"))
            .small()
            .color(egui::Color32::GRAY),
    );
}
