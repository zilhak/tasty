use std::sync::Arc;

use crate::i18n::t;
use crate::settings::{GeneralSettings, Settings};

/// Draw a label followed by a (?) icon with tooltip. For use inside Grid rows.
fn label_with_tooltip(ui: &mut egui::Ui, label: &str, tooltip: &str) {
    let th = crate::theme::theme();
    let text = egui::RichText::new(format!("{}  (?)", label));
    let response = ui.add(egui::Label::new(text).sense(egui::Sense::hover()));
    // Show tooltip only when hovering over the (?) portion
    if response.hovered() {
        response.show_tooltip_text(
            egui::RichText::new(tooltip).color(th.text),
        );
    }
}

pub fn draw_general_tab(ui: &mut egui::Ui, settings: &mut Settings) {
    let th = crate::theme::theme();
    ui.add_space(8.0);
    ui.heading(t("settings.general.heading"));
    ui.add_space(4.0);

    if !settings.general.is_shell_valid() {
        ui.label(
            egui::RichText::new(t("settings.general.shell_not_found"))
                .color(th.yellow),
        );
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
                    "fast" => t("settings.general.shell_mode_fast"),
                    "custom" => t("settings.general.shell_mode_custom"),
                    _ => t("settings.general.shell_mode_default"),
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut settings.general.shell_mode, "default".to_string(), t("settings.general.shell_mode_default"));
                    ui.selectable_value(&mut settings.general.shell_mode, "fast".to_string(), t("settings.general.shell_mode_fast"));
                    ui.selectable_value(&mut settings.general.shell_mode, "custom".to_string(), t("settings.general.shell_mode_custom"));
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
            ui.add(egui::DragValue::new(&mut settings.general.scrollback_lines)
                .range(0..=100000)
                .speed(100));
            ui.end_row();

            ui.label(t("settings.general.confirm_close_label"));
            ui.checkbox(&mut settings.general.confirm_close_running, "");
            ui.end_row();

            ui.label(t("settings.general.inherit_cwd_label"));
            ui.checkbox(&mut settings.general.inherit_cwd, "");
            ui.end_row();
        });
}

pub fn draw_appearance_tab(ui: &mut egui::Ui, settings: &mut Settings, font_families: &mut Option<Vec<String>>, font_filter: &mut String, preview_font_loaded: &mut String) {
    let th = crate::theme::theme();
    ui.add_space(8.0);

    // Left-right split: settings on the left, preview on the right
    ui.columns(2, |columns| {
        // ── Left column: settings controls ──
        columns[0].heading(t("settings.appearance.heading"));
        columns[0].add_space(4.0);

        egui::Grid::new("appearance_grid")
            .num_columns(2)
            .spacing([12.0, 8.0])
            .show(&mut columns[0], |ui| {
                // Font family: searchable combo box
                ui.label(t("settings.appearance.font_family_label"));
                let display_name = if settings.appearance.font_family.is_empty() {
                    "monospace (default)".to_string()
                } else {
                    settings.appearance.font_family.clone()
                };
                egui::ComboBox::from_id_salt("font_family_combo")
                    .selected_text(&display_name)
                    .width(200.0)
                    .height(300.0)
                    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                    .show_ui(ui, |ui| {
                        // Filter input
                        ui.add(
                            egui::TextEdit::singleline(font_filter)
                                .hint_text("Search...")
                                .desired_width(190.0),
                        );
                        ui.separator();

                        // Default monospace option
                        let filter_lower = font_filter.to_lowercase();
                        if filter_lower.is_empty() || "monospace".contains(&filter_lower) {
                            if ui
                                .selectable_label(
                                    settings.appearance.font_family.is_empty(),
                                    "monospace (default)",
                                )
                                .clicked()
                            {
                                settings.appearance.font_family.clear();
                            }
                        }

                        // System font list
                        if let Some(families) = &font_families {
                            egui::ScrollArea::vertical()
                                .max_height(250.0)
                                .show(ui, |ui| {
                                    for family in families {
                                        if !filter_lower.is_empty()
                                            && !family.to_lowercase().contains(&filter_lower)
                                        {
                                            continue;
                                        }
                                        let selected = settings.appearance.font_family == *family;
                                        if ui.selectable_label(selected, family).clicked() {
                                            settings.appearance.font_family = family.clone();
                                        }
                                    }
                                });
                        } else {
                            ui.label(
                                egui::RichText::new("Loading fonts...")
                                    .color(th.subtext0),
                            );
                        }
                    });
                ui.end_row();

                // Custom font file path
                ui.label("Custom font file:");
                ui.horizontal(|ui| {
                    ui.text_edit_singleline(&mut settings.appearance.custom_font_path);
                });
                ui.end_row();

                ui.label(t("settings.appearance.font_size_label"));
                ui.add(
                    egui::DragValue::new(&mut settings.appearance.font_size)
                        .range(6.0..=72.0)
                        .speed(0.5),
                );
                ui.end_row();

                label_with_tooltip(
                    ui,
                    "Line height:",
                    "Line height multiplier. 1.0 = tight (best for ASCII art), 1.2 = comfortable reading.",
                );
                ui.add(
                    egui::DragValue::new(&mut settings.appearance.line_height)
                        .range(0.8..=2.0)
                        .speed(0.05)
                        .max_decimals(2),
                );
                ui.end_row();

                ui.label(t("settings.appearance.background_opacity_label"));
                ui.add(egui::Slider::new(
                    &mut settings.appearance.background_opacity,
                    0.0..=1.0,
                ));
                ui.end_row();

                ui.label(t("settings.appearance.sidebar_width_label"));
                ui.add(
                    egui::DragValue::new(&mut settings.appearance.sidebar_width)
                        .range(100.0..=400.0)
                        .speed(1.0),
                );
                ui.end_row();

                label_with_tooltip(
                    ui,
                    t("settings.appearance.font_scale_mode_label"),
                    t("settings.appearance.font_scale_mode_tooltip"),
                );
                egui::ComboBox::from_id_salt("font_scale_mode")
                    .selected_text(match settings.appearance.font_scale_mode.as_str() {
                        "auto" => t("settings.appearance.font_scale_mode_auto"),
                        _ => t("settings.appearance.font_scale_mode_fixed"),
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut settings.appearance.font_scale_mode,
                            "auto".to_string(),
                            t("settings.appearance.font_scale_mode_auto"),
                        );
                        ui.selectable_value(
                            &mut settings.appearance.font_scale_mode,
                            "fixed".to_string(),
                            t("settings.appearance.font_scale_mode_fixed"),
                        );
                    });
                ui.end_row();

                ui.label("UI Scale");
                egui::ComboBox::from_id_salt("ui_scale")
                    .selected_text(match settings.appearance.ui_scale.as_str() {
                        "small" => "Small",
                        "large" => "Large",
                        _ => "Medium",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut settings.appearance.ui_scale,
                            "small".to_string(),
                            "Small",
                        );
                        ui.selectable_value(
                            &mut settings.appearance.ui_scale,
                            "medium".to_string(),
                            "Medium",
                        );
                        ui.selectable_value(
                            &mut settings.appearance.ui_scale,
                            "large".to_string(),
                            "Large",
                        );
                    });
                ui.end_row();
            });

        // ── Right column: font preview ──
        draw_font_preview(&mut columns[1], settings, th, preview_font_loaded);
    });
}

/// Draw a fake terminal preview showing the current font/appearance settings.
fn draw_font_preview(ui: &mut egui::Ui, settings: &Settings, th: &crate::theme::Theme, preview_font_loaded: &mut String) {
    ui.heading("Preview");
    ui.add_space(4.0);

    let font_name = if settings.appearance.font_family.is_empty() {
        "monospace"
    } else {
        &settings.appearance.font_family
    };
    let font_size = settings.appearance.font_size;

    // Load selected font into egui if it changed.
    // `preview_font_loaded` holds either:
    //   - the font family name on success (matches font_family → already loaded)
    //   - "\x00:<font_family>" as a failure marker (don't retry)
    //   - "" on init (never attempted)
    let failed_marker = format!("\x00:{}", settings.appearance.font_family);
    let preview_family = if settings.appearance.font_family.is_empty() {
        egui::FontFamily::Monospace
    } else if *preview_font_loaded == settings.appearance.font_family {
        // Already loaded successfully.
        egui::FontFamily::Name("preview".into())
    } else if *preview_font_loaded == failed_marker {
        // Load was already attempted and failed; don't retry.
        egui::FontFamily::Monospace
    } else {
        // First attempt for this font family.
        let font_config = crate::font::FontConfig::new(14.0, "");
        if let Some(data) = font_config.load_family_data(&settings.appearance.font_family) {
            let mut fonts = egui::FontDefinitions::default();
            fonts.font_data.insert(
                "preview_font".to_owned(),
                Arc::new(egui::FontData::from_owned(data)),
            );
            fonts
                .families
                .insert(
                    egui::FontFamily::Name("preview".into()),
                    vec!["preview_font".to_owned()],
                );
            // Keep CJK fallback for Monospace/Proportional (re-run CJK setup)
            if let Some(cjk_data) = load_system_cjk_font_data() {
                fonts.font_data.insert(
                    "system_cjk".to_owned(),
                    Arc::new(egui::FontData::from_owned(cjk_data)),
                );
                fonts
                    .families
                    .entry(egui::FontFamily::Proportional)
                    .or_default()
                    .push("system_cjk".to_owned());
                fonts
                    .families
                    .entry(egui::FontFamily::Monospace)
                    .or_default()
                    .push("system_cjk".to_owned());
                fonts
                    .families
                    .entry(egui::FontFamily::Name("preview".into()))
                    .or_default()
                    .push("system_cjk".to_owned());
            }
            ui.ctx().set_fonts(fonts);
            *preview_font_loaded = settings.appearance.font_family.clone();
            egui::FontFamily::Name("preview".into())
        } else {
            // Record failure so we don't retry on subsequent frames.
            *preview_font_loaded = failed_marker;
            egui::FontFamily::Monospace
        }
    };

    let sample_lines = [
        "AaBbCcDdEeFfGg",
        "\u{AC00}\u{B098}\u{B2E4}\u{B77C}\u{B9C8}\u{BC14}\u{C0AC}",       // 가나다라마바사
        "1234567890",
        "\u{30A2}\u{30AB}\u{30B5}\u{30BF}\u{30CA}\u{30CF}\u{30DE}\u{30E9}\u{30E4}\u{30EF}", // アカサタナハマラヤワ
    ];

    let focused_bg = settings.appearance.focused_surface_bg_float();
    let unfocused_bg = th.terminal_bg;
    let fg = th.terminal_fg;

    let focused_bg32 = egui::Color32::from_rgb(
        (focused_bg[0] * 255.0) as u8,
        (focused_bg[1] * 255.0) as u8,
        (focused_bg[2] * 255.0) as u8,
    );
    let unfocused_bg32 = egui::Color32::from_rgb(
        (unfocused_bg[0] * 255.0) as u8,
        (unfocused_bg[1] * 255.0) as u8,
        (unfocused_bg[2] * 255.0) as u8,
    );
    let fg32 = egui::Color32::from_rgb(
        (fg[0] * 255.0) as u8,
        (fg[1] * 255.0) as u8,
        (fg[2] * 255.0) as u8,
    );

    let preview_font = egui::FontId::new(font_size, preview_family);
    let line_height = font_size * 1.4;
    let padding = 8.0;
    let block_height = line_height * sample_lines.len() as f32 + padding * 2.0;

    // ── Focused preview ──
    ui.label(
        egui::RichText::new("Focused")
            .size(th.font_size_caption)
            .color(th.subtext0),
    );
    let (focused_rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), block_height),
        egui::Sense::hover(),
    );
    ui.painter().rect_filled(focused_rect, 2.0, focused_bg32);
    // Focused border highlight
    ui.painter().rect_stroke(
        focused_rect,
        2.0,
        egui::Stroke::new(th.border_width, th.blue),
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
        egui::RichText::new("Unfocused")
            .size(th.font_size_caption)
            .color(th.subtext0),
    );
    let (unfocused_rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), block_height),
        egui::Sense::hover(),
    );
    ui.painter().rect_filled(unfocused_rect, 2.0, unfocused_bg32);
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
        egui::RichText::new(format!("Font: {} / Size: {:.1}px", font_name, font_size))
            .size(th.font_size_caption)
            .color(th.subtext0),
    );
}

/// Load system CJK font data for egui fallback (mirrors GpuState::load_system_cjk_font).
fn load_system_cjk_font_data() -> Option<Vec<u8>> {
    #[cfg(target_os = "windows")]
    {
        let path = "C:/Windows/Fonts/malgun.ttf";
        if let Ok(data) = std::fs::read(path) {
            return Some(data);
        }
    }

    #[cfg(target_os = "macos")]
    {
        for path in &[
            "/System/Library/Fonts/AppleSDGothicNeo.ttc",
            "/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc",
            "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
        ] {
            if let Ok(data) = std::fs::read(path) {
                return Some(data);
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        for path in &[
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
        ] {
            if let Ok(data) = std::fs::read(path) {
                return Some(data);
            }
        }
    }

    None
}

pub fn draw_clipboard_tab(ui: &mut egui::Ui, settings: &mut Settings) {
    ui.add_space(8.0);
    ui.heading(t("settings.clipboard.heading"));
    ui.add_space(4.0);

    ui.checkbox(&mut settings.clipboard.macos_style, t("settings.clipboard.macos_style"));
    ui.checkbox(&mut settings.clipboard.linux_style, t("settings.clipboard.linux_style"));
    ui.checkbox(&mut settings.clipboard.windows_style, t("settings.clipboard.windows_style"));

    ui.add_space(12.0);
    ui.heading(t("settings.zoom.heading"));
    ui.add_space(4.0);

    ui.checkbox(&mut settings.zoom.ctrl_style, t("settings.zoom.ctrl_style"));
    ui.checkbox(&mut settings.zoom.alt_style, t("settings.zoom.alt_style"));
}

pub fn draw_notifications_tab(ui: &mut egui::Ui, settings: &mut Settings) {
    ui.add_space(8.0);
    ui.heading(t("settings.notifications.heading"));
    ui.add_space(4.0);

    ui.checkbox(&mut settings.notification.enabled, t("settings.notifications.enabled"));
    ui.checkbox(&mut settings.notification.system_notification, t("settings.notifications.system_notification"));
    ui.checkbox(&mut settings.notification.sound, t("settings.notifications.sound"));

    ui.add_space(8.0);
    egui::Grid::new("notification_grid")
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            ui.label(t("settings.notifications.coalesce_interval_label"));
            ui.add(egui::DragValue::new(&mut settings.notification.coalesce_ms)
                .range(0..=5000)
                .speed(50));
            ui.end_row();
        });
}

pub fn draw_language_tab(ui: &mut egui::Ui, settings: &mut Settings) {
    let th = crate::theme::theme();
    ui.add_space(8.0);
    ui.heading(t("settings.language.heading"));
    ui.add_space(4.0);

    egui::Grid::new("language_grid")
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            ui.label(t("settings.language.label"));
            egui::ComboBox::from_id_salt("language_select")
                .selected_text(language_display_name(&settings.general.language))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut settings.general.language, "en".to_string(), "English");
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

pub fn draw_performance_tab(ui: &mut egui::Ui, settings: &mut Settings) {
    let th = crate::theme::theme();
    ui.heading("Performance");
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new("Changes require restart to take effect.")
            .small()
            .color(th.yellow),
    );
    ui.add_space(12.0);

    ui.checkbox(
        &mut settings.performance.targeted_pty_polling,
        "Targeted PTY polling",
    );
    ui.label(
        egui::RichText::new("Only process terminals with new output instead of polling all. Reduces CPU with many surfaces.")
            .small()
            .color(egui::Color32::GRAY),
    );
    ui.add_space(8.0);

    ui.checkbox(
        &mut settings.performance.scrollback_disk_swap,
        "Scrollback disk swap",
    );
    ui.label(
        egui::RichText::new("Swap old scrollback lines to disk to reduce memory usage.")
            .small()
            .color(egui::Color32::GRAY),
    );
    ui.add_space(8.0);

    ui.checkbox(
        &mut settings.performance.lazy_pty_init,
        "Lazy PTY initialization",
    );
    ui.label(
        egui::RichText::new("Spawn shell processes only when a tab is first focused, not at creation.")
            .small()
            .color(egui::Color32::GRAY),
    );
}
