mod keybindings_tab;
mod tabs;

use crate::i18n::t;
use crate::settings::Settings;
use crate::ui::popup::{PopupManager, PopupState};

use keybindings_tab::{KeybindingsSubTab, PendingBinding, RecordingSlot, draw_keybindings_tab};
use tabs::*;

/// Sub-tab within the Appearance tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AppearanceSubTab {
    Theme,
    General,
    Tasty,
    Terminal,
    Markdown,
    Explorer,
    HtmlViewer,
}

/// Sub-tab within the Misc tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MiscSubTab {
    Tastyrc,
}

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
    Misc,
}

/// Persistent state for the settings UI between frames.
pub struct SettingsUiState {
    active_tab: SettingsTab,
    /// Working copy of settings being edited.
    draft: Option<Settings>,
    /// Which keybinding field+slot is currently recording input (None = not recording).
    recording_field: Option<RecordingSlot>,
    /// Active sub-tab within keybindings.
    keybindings_sub_tab: KeybindingsSubTab,
    /// Active sub-tab within appearance.
    appearance_sub_tab: AppearanceSubTab,
    /// Active sub-tab within misc.
    misc_sub_tab: MiscSubTab,
    /// Currently previewed preset name in the Preset sub-tab (None = no preview).
    selected_preset: Option<String>,
    /// Pending keybinding assignment waiting for conflict confirmation.
    pending_binding: Option<PendingBinding>,
    /// Popup manager for settings-window popups (e.g. keybinding conflict).
    popups: PopupManager,
    /// 충돌 팝업에서 수락/거부 결과를 전달하는 플래그.
    conflict_accepted: bool,
    conflict_cancelled: bool,
    /// Cached system font family list.
    pub font_families: Option<Vec<String>>,
    /// Font family filter text for search.
    pub font_filter: String,
    /// The font family currently loaded into egui for preview.
    pub preview_font_loaded: String,
    /// Draft of ~/.tasty/bashrc.user content. None until the Misc tab loads it.
    pub(crate) bashrc_user_draft: Option<String>,
}

impl SettingsUiState {
    pub fn new() -> Self {
        let mut popups = PopupManager::new();
        popups.register(
            PopupState::new(
                "keybinding_conflict",
                t("settings.keybindings.conflict_title"),
                egui::vec2(340.0, 120.0),
            )
            .with_close_on_outside_click(false),
        );
        Self {
            active_tab: SettingsTab::General,
            draft: None,
            recording_field: None,
            keybindings_sub_tab: KeybindingsSubTab::General,
            appearance_sub_tab: AppearanceSubTab::General,
            misc_sub_tab: MiscSubTab::Tastyrc,
            selected_preset: None,
            pending_binding: None,
            popups,
            conflict_accepted: false,
            conflict_cancelled: false,
            font_families: None,
            font_filter: String::new(),
            preview_font_loaded: String::new(),
            bashrc_user_draft: None,
        }
    }
}

/// Draw settings directly as a full-window panel (for modal windows).
/// Returns true if Save was clicked, false if Cancel was clicked, None otherwise.
pub fn draw_settings_panel(
    ctx: &egui::Context,
    settings: &mut Settings,
    ui_state: &mut SettingsUiState,
    captured_double_tap: &mut Option<String>,
) -> Option<bool> {
    if ui_state.draft.is_none() {
        ui_state.draft = Some(settings.clone());
    }

    // Lazily load system font list on first access
    if ui_state.font_families.is_none() {
        let font_config = crate::font::FontConfig::new(14.0, "");
        ui_state.font_families = Some(font_config.list_families());
    }

    // Lazily load ~/.tasty/bashrc.user on first settings open
    if ui_state.bashrc_user_draft.is_none() {
        ui_state.bashrc_user_draft = Some(crate::settings::general::load_user_bashrc());
    }

    let mut result = None;

    egui::TopBottomPanel::bottom("settings_buttons").show(ctx, |ui| {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(t("button.cancel")).clicked() {
                    // Cancel: discard bashrc draft so next open reloads from disk.
                    ui_state.bashrc_user_draft = None;
                    result = Some(false);
                }
                if ui.button(t("button.save")).clicked() {
                    if let Some(draft) = &ui_state.draft {
                        *settings = draft.clone();
                    }
                    if let Some(bashrc) = &ui_state.bashrc_user_draft {
                        crate::settings::general::save_user_bashrc(bashrc);
                    }
                    // Apply the selected theme preset at runtime
                    let presets = crate::theme::presets();
                    if let Some(preset) = presets.iter().find(|p| p.id == settings.appearance.theme) {
                        crate::theme::set_theme(preset.theme);
                    }
                    result = Some(true);
                }
            });
        });
        ui.add_space(4.0);
    });

    egui::CentralPanel::default().show(ctx, |ui| {
        ui.add_space(8.0);

        ui.horizontal(|ui| {
            let tabs = [
                (SettingsTab::General, t("settings.tab.general")),
                (SettingsTab::Appearance, t("settings.tab.appearance")),
                (SettingsTab::Clipboard, t("settings.tab.clipboard")),
                (SettingsTab::Notifications, t("settings.tab.notifications")),
                (SettingsTab::Keybindings, t("settings.tab.keybindings")),
                (SettingsTab::Language, t("settings.tab.language")),
                (SettingsTab::Performance, t("settings.performance.heading")),
                (SettingsTab::Misc, t("settings.tab.misc")),
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
                .show(ui, |ui| match active_tab {
                    SettingsTab::General => draw_general_tab(ui, &mut draft),
                    SettingsTab::Appearance => draw_appearance_tab(
                        ui,
                        &mut draft,
                        &mut ui_state.appearance_sub_tab,
                        &mut ui_state.font_families,
                        &mut ui_state.font_filter,
                        &mut ui_state.preview_font_loaded,
                    ),
                    SettingsTab::Clipboard => draw_clipboard_tab(ui, &mut draft),
                    SettingsTab::Notifications => draw_notifications_tab(ui, &mut draft),
                    SettingsTab::Keybindings => draw_keybindings_tab(
                        ui,
                        &mut draft,
                        &mut ui_state.recording_field,
                        &mut ui_state.keybindings_sub_tab,
                        &mut ui_state.selected_preset,
                        &mut ui_state.pending_binding,
                        captured_double_tap,
                    ),
                    SettingsTab::Language => draw_language_tab(ui, &mut draft),
                    SettingsTab::Performance => draw_performance_tab(ui, &mut draft),
                    SettingsTab::Misc => draw_misc_tab(
                        ui,
                        &mut ui_state.misc_sub_tab,
                        &mut ui_state.bashrc_user_draft,
                    ),
                });

            // 충돌 감지 시 팝업 열기
            if ui_state.pending_binding.is_some()
                && !ui_state.popups.is_open("keybinding_conflict")
            {
                ui_state.popups.open_centered_focused("keybinding_conflict");
            }

            // 충돌 팝업에서 수락/거부 처리
            if ui_state.conflict_accepted {
                ui_state.conflict_accepted = false;
                if let Some(pending) = ui_state.pending_binding.take() {
                    draft
                        .keybindings
                        .remove_binding(&pending.conflicting_field, pending.conflicting_idx);
                    draft.keybindings.replace_binding_at(
                        &pending.target_field,
                        pending.target_idx,
                        pending.combo,
                    );
                }
                ui_state.popups.close("keybinding_conflict");
            }
            if ui_state.conflict_cancelled {
                ui_state.conflict_cancelled = false;
                ui_state.pending_binding = None;
                ui_state.popups.close("keybinding_conflict");
            }

            ui_state.draft = Some(draft);
        }
    });

    // Draw popups (충돌 확인 등)
    let popup_result = {
        let pending = ui_state.pending_binding.clone();
        let accepted = &mut ui_state.conflict_accepted;
        let cancelled = &mut ui_state.conflict_cancelled;
        ui_state.popups.draw(
            ctx,
            &mut |id, ui| {
                if id == "keybinding_conflict" {
                    if let Some(pending) = &pending {
                        let conflict_label_raw =
                            crate::settings::KeybindingSettings::label_key_for(
                                &pending.conflicting_field,
                            )
                            .map(t)
                            .unwrap_or(pending.conflicting_field.as_str());
                        let conflict_label =
                            conflict_label_raw.trim_end_matches(':').trim().to_string();
                        let combo_display =
                            crate::settings::KeybindingSettings::format_display(&pending.combo);

                        ui.label(crate::i18n::t_fmt2(
                            "settings.keybindings.conflict_message",
                            &combo_display,
                            &conflict_label,
                        ));
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if ui.button(t("button.cancel")).clicked() {
                                *cancelled = true;
                            }
                            if ui
                                .button(t("settings.keybindings.conflict_apply"))
                                .clicked()
                            {
                                *accepted = true;
                            }
                        });
                    }
                }
            },
            None,
        )
    };

    // X 버튼으로 충돌 팝업이 닫힌 경우 pending_binding 정리
    if popup_result.closed.contains(&"keybinding_conflict") {
        ui_state.pending_binding = None;
    }

    // 키보드로 충돌 팝업 수락/거부
    if ui_state.popups.is_open("keybinding_conflict") {
        ctx.input(|i| {
            if i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Y) {
                ui_state.conflict_accepted = true;
            }
            if i.key_pressed(egui::Key::Escape) || i.key_pressed(egui::Key::N) {
                ui_state.conflict_cancelled = true;
            }
        });
    }

    result
}
