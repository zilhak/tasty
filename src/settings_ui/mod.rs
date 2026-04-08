mod keybindings_tab;
mod tabs;

use crate::i18n::t;
use crate::settings::Settings;

use keybindings_tab::{draw_keybindings_tab, KeybindingsSubTab};
use tabs::*;

/// Active tab in the settings window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SettingsTab {
    General,
    Appearance,
    Clipboard,
    Notifications,
    Keybindings,
    Language,
    Performance,
}

/// Persistent state for the settings UI between frames.
pub struct SettingsUiState {
    active_tab: SettingsTab,
    /// Working copy of settings being edited.
    draft: Option<Settings>,
    /// Which keybinding field is currently recording input (None = not recording).
    recording_field: Option<String>,
    /// Active sub-tab within keybindings.
    keybindings_sub_tab: KeybindingsSubTab,
    /// Pending preset name to apply (waiting for user confirmation).
    preset_confirm: Option<String>,
    /// Cached system font family list.
    pub font_families: Option<Vec<String>>,
    /// Font family filter text for search.
    pub font_filter: String,
}

impl SettingsUiState {
    pub fn new() -> Self {
        Self {
            active_tab: SettingsTab::General,
            draft: None,
            recording_field: None,
            keybindings_sub_tab: KeybindingsSubTab::General,
            preset_confirm: None,
            font_families: None,
            font_filter: String::new(),
        }
    }
}

/// Draw settings directly as a full-window panel (for modal windows).
/// Returns true if Save was clicked, false if Cancel was clicked, None otherwise.
pub fn draw_settings_panel(
    ctx: &egui::Context,
    settings: &mut Settings,
    ui_state: &mut SettingsUiState,
) -> Option<bool> {
    if ui_state.draft.is_none() {
        ui_state.draft = Some(settings.clone());
    }

    // Lazily load system font list on first access
    if ui_state.font_families.is_none() {
        let font_config = crate::font::FontConfig::new(14.0, "");
        ui_state.font_families = Some(font_config.list_families());
    }

    let mut result = None;

    egui::TopBottomPanel::bottom("settings_buttons").show(ctx, |ui| {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(t("button.cancel")).clicked() {
                    result = Some(false);
                }
                if ui.button(t("button.save")).clicked() {
                    if let Some(draft) = &ui_state.draft {
                        *settings = draft.clone();
                    }
                    result = Some(true);
                }
            });
        });
        ui.add_space(4.0);
    });

    egui::CentralPanel::default().show(ctx, |ui| {
        ui.add_space(8.0);
        ui.heading(t("settings.window.title"));
        ui.add_space(8.0);

        ui.horizontal(|ui| {
            let tabs = [
                (SettingsTab::General, t("settings.tab.general")),
                (SettingsTab::Appearance, t("settings.tab.appearance")),
                (SettingsTab::Clipboard, t("settings.tab.clipboard")),
                (SettingsTab::Notifications, t("settings.tab.notifications")),
                (SettingsTab::Keybindings, t("settings.tab.keybindings")),
                (SettingsTab::Language, t("settings.tab.language")),
                (SettingsTab::Performance, "Performance"),
            ];
            for (tab, label) in &tabs {
                let selected = ui_state.active_tab == *tab;
                if ui.selectable_label(selected, *label).clicked() {
                    ui_state.active_tab = *tab;
                }
            }
        });
        ui.separator();

        {
            let mut draft = ui_state.draft.take().unwrap();
            let active_tab = ui_state.active_tab;

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    match active_tab {
                        SettingsTab::General => draw_general_tab(ui, &mut draft),
                        SettingsTab::Appearance => draw_appearance_tab(ui, &mut draft, &mut ui_state.font_families, &mut ui_state.font_filter),
                        SettingsTab::Clipboard => draw_clipboard_tab(ui, &mut draft),
                        SettingsTab::Notifications => draw_notifications_tab(ui, &mut draft),
                        SettingsTab::Keybindings => draw_keybindings_tab(ui, &mut draft, &mut ui_state.recording_field, &mut ui_state.keybindings_sub_tab, &mut ui_state.preset_confirm),
                        SettingsTab::Language => draw_language_tab(ui, &mut draft),
                        SettingsTab::Performance => draw_performance_tab(ui, &mut draft),
                    }
                });

            ui_state.draft = Some(draft);
        }
    });

    result
}
