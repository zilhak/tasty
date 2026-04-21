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
                    "tasty" => t("settings.general.shell_mode_tasty"),
                    "custom" => t("settings.general.shell_mode_custom"),
                    _ => t("settings.general.shell_mode_default"),
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut settings.general.shell_mode, "default".to_string(), t("settings.general.shell_mode_default"));
                    ui.selectable_value(&mut settings.general.shell_mode, "tasty".to_string(), t("settings.general.shell_mode_tasty"));
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

            ui.label(t("settings.general.close_behavior_label"));
            egui::ComboBox::from_id_salt("close_behavior")
                .selected_text(match settings.general.close_behavior.as_str() {
                    "quit" => t("settings.general.close_behavior_quit"),
                    "minimize" => t("settings.general.close_behavior_minimize"),
                    _ => t("settings.general.close_behavior_ask"),
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut settings.general.close_behavior, "ask".to_string(), t("settings.general.close_behavior_ask"));
                    ui.selectable_value(&mut settings.general.close_behavior, "minimize".to_string(), t("settings.general.close_behavior_minimize"));
                    ui.selectable_value(&mut settings.general.close_behavior, "quit".to_string(), t("settings.general.close_behavior_quit"));
                });
            ui.end_row();
        });
}

pub fn draw_appearance_tab(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    sub_tab: &mut super::AppearanceSubTab,
    font_families: &mut Option<Vec<String>>,
    font_filter: &mut String,
    preview_font_loaded: &mut String,
) {
    use super::AppearanceSubTab;
    let th = crate::theme::theme();
    ui.add_space(8.0);

    let available_height = ui.available_height() - 8.0;

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
                        (AppearanceSubTab::General, t("settings.appearance.subtab.general")),
                        (AppearanceSubTab::Tasty, "Tasty"),
                        (AppearanceSubTab::Terminal, t("settings.appearance.subtab.terminal")),
                        (AppearanceSubTab::Markdown, t("settings.appearance.subtab.markdown")),
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
        ui.vertical(|ui| {
            match *sub_tab {
                AppearanceSubTab::General => {
                    draw_appearance_general(ui, settings);
                }
                AppearanceSubTab::Tasty => {
                    draw_appearance_tasty(ui, settings);
                }
                AppearanceSubTab::Terminal => {
                    draw_appearance_terminal(ui, settings, font_families, font_filter, preview_font_loaded);
                }
                AppearanceSubTab::Markdown => {
                    draw_appearance_placeholder(ui, "Markdown");
                }
                AppearanceSubTab::Explorer => {
                    draw_appearance_placeholder(ui, "Explorer");
                }
                AppearanceSubTab::HtmlViewer => {
                    draw_appearance_placeholder(ui, "HTML Viewer");
                }
            }
        });
    });
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

/// Appearance > Terminal: font, font size, line height, DPI mode, preview
fn draw_appearance_terminal(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    font_families: &mut Option<Vec<String>>,
    font_filter: &mut String,
    preview_font_loaded: &mut String,
) {
    let th = crate::theme::theme();

    ui.columns(2, |columns| {
        // ── Left: terminal font settings ──
        egui::Grid::new("appearance_terminal_grid")
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
                        ui.add(
                            egui::TextEdit::singleline(font_filter)
                                .hint_text(t("settings.appearance.search_hint"))
                                .desired_width(190.0),
                        );
                        ui.separator();

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
                                egui::RichText::new(t("settings.appearance.loading_fonts"))
                                    .color(th.subtext0),
                            );
                        }
                    });
                ui.end_row();

                ui.label(t("settings.appearance.custom_font_label"));
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
                    t("settings.appearance.line_height_label"),
                    t("settings.appearance.line_height_tooltip"),
                );
                ui.add(
                    egui::DragValue::new(&mut settings.appearance.line_height)
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
            });

        // ── Right: font preview ──
        draw_font_preview(&mut columns[1], settings, th, preview_font_loaded);
    });
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

/// Draw a fake terminal preview showing the current font/appearance settings.
fn draw_font_preview(ui: &mut egui::Ui, settings: &Settings, th: &crate::theme::Theme, preview_font_loaded: &mut String) {
    ui.heading(t("settings.appearance.preview_heading"));
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
            // set_fonts() was just called this frame; the new family may not be
            // available until the next frame. Use Monospace now and switch to
            // Name("preview") from the next frame (preview_font_loaded is already set).
            egui::FontFamily::Monospace
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
        egui::RichText::new(t("settings.appearance.preview_focused"))
            .size(th.font_size_caption.value())
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
        egui::RichText::new(
            crate::i18n::t_fmt("settings.appearance.preview_font_info", &format!("{} / {:.1}px", font_name, font_size))
        )
            .size(th.font_size_caption.value())
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

/// Misc / 기타 탭: 좌측 서브탭 메뉴 + 우측 콘텐츠.
pub fn draw_misc_tab(
    ui: &mut egui::Ui,
    sub_tab: &mut crate::settings_ui::MiscSubTab,
    bashrc_user_draft: &mut Option<String>,
) {
    let th = crate::theme::theme();
    ui.add_space(8.0);

    let available_height = ui.available_height() - 8.0;

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
                    let sub_tabs = [
                        (
                            crate::settings_ui::MiscSubTab::Tastyrc,
                            t("settings.misc.subtab.tastyrc"),
                        ),
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
