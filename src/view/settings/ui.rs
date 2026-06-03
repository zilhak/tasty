mod file_handler_tab;
mod keybindings_tab;
mod tabs;

use file_handler_tab::{FileHandlerSubTab, draw_file_handler_tab};
use keybindings_tab::{KeybindingsSubTab, PendingBinding, RecordingSlot, draw_keybindings_tab};
use tabs::*;

pub use keybindings_tab::{KeyCapture, capture_winit_key_combo};

use crate::adapters::ui::popup::{PopupManager, PopupState};
use crate::file::format::{DetectorId, FileFormatRegistry};
use crate::file::handler::FileHandlerRegistry;
use crate::i18n::t;
use crate::plugin::manifest::BindingMode;
use crate::plugin::registry_state::ShortcutOverride;
use crate::settings::Settings;

/// 단계 E: Plugins 서브탭에서 표시할 한 row.
///
/// `current_override`는 사용자가 plugins.toml에 저장해 둔 값 (없으면 매니페스트
/// default 사용). UI는 read-only 표시이므로 변경은 다음 단계에서 추가.
#[derive(Debug, Clone)]
pub struct PluginShortcutRow {
    pub plugin_id: String,
    pub plugin_name: String,
    pub command_id: String,
    pub title_i18n_key: String,
    pub binding_mode: BindingMode,
    pub manifest_default: Option<String>,
    pub current_override: Option<ShortcutOverride>,
}

#[derive(Debug, Default, Clone)]
pub struct PluginShortcutSnapshot {
    pub rows: Vec<PluginShortcutRow>,
}

/// Sub-tab within the Appearance tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AppearanceSubTab {
    Theme,
    General,
    Tasty,
    Terminal,
    Markdown,
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
    Terminal,
    Appearance,
    Clipboard,
    Notifications,
    Keybindings,
    Language,
    Performance,
    Accessibility,
    FileHandler,
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
    /// Active sub-tab within FileHandler.
    file_handler_sub_tab: FileHandlerSubTab,
    /// FileHandler 탭의 Extension Mapping draft. None 이면 첫 진입 시 registry 에서 초기화.
    /// 키 = 확장자 (소문자, 점 없음), 값 = 정렬된 detector id 리스트 (빈 리스트 = 클리어).
    pub(crate) extension_priority_draft:
        Option<std::collections::BTreeMap<String, Vec<DetectorId>>>,
    /// 사용자가 새 확장자 추가 시 입력하는 텍스트 (Extension Mapping sub-tab).
    pub(crate) extension_priority_new_input: String,
    /// FileHandler 탭의 Detectors/Handlers sub-tab 편집 draft. Save 시 registry 에 commit +
    /// 디스크 저장. Cancel 시 폐기.
    pub(crate) fh_edit_draft: file_handler_tab::FileHandlerEditDraft,
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
    /// Font family filter text for search (per font picker, keyed by slot id).
    pub font_filter: std::collections::HashMap<String, String>,
    /// Family currently loaded into each preview slot ("preview_default", etc.).
    /// A `\0:<family>` value records a previous load failure to avoid retry loops.
    pub preview_font_loaded: std::collections::HashMap<String, String>,
    /// Draft of ~/.tasty/bashrc.user content. None until the Misc tab loads it.
    pub(crate) bashrc_user_draft: Option<String>,
    /// winit KeyboardInput에서 직접 캡처한 키 조합 (녹화 중일 때 사용).
    pub captured_winit_combo: Option<KeyCapture>,
    /// Plugins 서브탭이 표시할 plugin command snapshot (모달 오픈 시 1회 채워짐).
    pub plugin_shortcuts: PluginShortcutSnapshot,
    /// Plugins 서브탭에서 현재 선택된 plugin id.
    pub plugin_shortcuts_selected: Option<String>,
    /// 사용자가 Plugins 서브탭에서 변경한 override draft.
    /// 키 = (plugin_id, command_id), 값:
    /// - `Some(ShortcutOverride)`: 새 override 적용
    /// - `None`: clear (매니페스트 default로 복귀)
    ///
    /// 모달 close 시 main App이 회수해 `PluginsConfig.keybindings`에 반영하고
    /// 디스크에 저장한다.
    pub plugin_shortcuts_draft:
        std::collections::BTreeMap<(String, String), Option<ShortcutOverride>>,
}

impl SettingsUiState {
    /// 단축키 녹화 중인지 여부.
    pub fn is_recording(&self) -> bool {
        self.recording_field.is_some()
    }

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
            file_handler_sub_tab: FileHandlerSubTab::ExtensionMapping,
            extension_priority_draft: None,
            extension_priority_new_input: String::new(),
            fh_edit_draft: file_handler_tab::FileHandlerEditDraft::default(),
            selected_preset: None,
            pending_binding: None,
            popups,
            conflict_accepted: false,
            conflict_cancelled: false,
            font_families: None,
            font_filter: std::collections::HashMap::new(),
            preview_font_loaded: std::collections::HashMap::new(),
            bashrc_user_draft: None,
            captured_winit_combo: None,
            plugin_shortcuts: PluginShortcutSnapshot::default(),
            plugin_shortcuts_selected: None,
            plugin_shortcuts_draft: std::collections::BTreeMap::new(),
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
    file_format: &FileFormatRegistry,
    file_handler: &FileHandlerRegistry,
    user_config_path: Option<&std::path::Path>,
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
                    ui_state.extension_priority_draft = None;
                    ui_state.fh_edit_draft = file_handler_tab::FileHandlerEditDraft::default();
                    result = Some(false);
                }
                if ui.button(t("button.save")).clicked() {
                    let prev_restore_terminal_content = settings.general.restore_terminal_content;
                    if let Some(draft) = &ui_state.draft {
                        *settings = draft.clone();
                    }
                    // restore_terminal_content 를 끈 경우 기존에 쌓인 scrollback
                    // 파일을 모두 정리 (사용자가 더 이상 안 쓴다고 명시).
                    if prev_restore_terminal_content && !settings.general.restore_terminal_content {
                        crate::scrollback_store::clear_all();
                    }
                    if let Some(bashrc) = &ui_state.bashrc_user_draft {
                        crate::settings::general::save_user_bashrc(bashrc);
                    }
                    // 선택된 테마 즉시 적용 — 설정 화면에서 콤보 선택은 이미 apply_theme 으로
                    // base/overrides 를 갱신했고, 여기서는 전역 Theme 인스턴스만 install.
                    tasty_themes::install_global(&settings.appearance);
                    // FileHandler 탭의 Extension Mapping + Detectors/Handlers 편집 draft 를
                    // registry 에 commit + 디스크 저장.
                    let mut fh_touched = false;
                    if let Some(draft) = ui_state.extension_priority_draft.take() {
                        for (ext, order) in &draft {
                            if order.is_empty() {
                                file_format.clear_user_extension_priority(ext);
                            } else {
                                file_format.set_user_extension_priority(ext, order.clone());
                            }
                        }
                        fh_touched = true;
                    }
                    {
                        let fh = std::mem::take(&mut ui_state.fh_edit_draft);
                        if fh.has_changes() {
                            fh.apply(file_format, file_handler);
                            fh_touched = true;
                        }
                    }
                    if fh_touched
                        && let Some(path) = user_config_path
                        && let Err(e) = crate::file::handler::save::save_combined_user_config(
                            file_format,
                            file_handler,
                            path,
                        )
                    {
                        tracing::warn!("file_handler tab: save_combined_user_config failed: {e}");
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
                (SettingsTab::Terminal, t("settings.tab.terminal")),
                (SettingsTab::Appearance, t("settings.tab.appearance")),
                (SettingsTab::Clipboard, t("settings.tab.clipboard")),
                (SettingsTab::Notifications, t("settings.tab.notifications")),
                (SettingsTab::Keybindings, t("settings.tab.keybindings")),
                (SettingsTab::Language, t("settings.tab.language")),
                (SettingsTab::Performance, t("settings.performance.heading")),
                (SettingsTab::Accessibility, t("settings.tab.accessibility")),
                (SettingsTab::FileHandler, t("settings.tab.file_handler")),
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
                .drag_to_scroll(false)
                .show(ui, |ui| match active_tab {
                    SettingsTab::General => draw_general_tab(ui, &mut draft),
                    SettingsTab::Terminal => draw_terminal_tab(ui, &mut draft),
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
                        &mut ui_state.captured_winit_combo,
                        &ui_state.plugin_shortcuts,
                        &mut ui_state.plugin_shortcuts_selected,
                        &mut ui_state.plugin_shortcuts_draft,
                    ),
                    SettingsTab::Language => draw_language_tab(ui, &mut draft),
                    SettingsTab::Performance => draw_performance_tab(ui, &mut draft),
                    SettingsTab::Accessibility => draw_accessibility_tab(ui, &mut draft),
                    SettingsTab::FileHandler => draw_file_handler_tab(
                        ui,
                        &mut ui_state.file_handler_sub_tab,
                        &mut ui_state.extension_priority_draft,
                        &mut ui_state.extension_priority_new_input,
                        &mut ui_state.fh_edit_draft,
                        file_format,
                        file_handler,
                    ),
                    SettingsTab::Misc => draw_misc_tab(
                        ui,
                        &mut ui_state.misc_sub_tab,
                        &mut ui_state.bashrc_user_draft,
                    ),
                });

            // 충돌 감지 시 팝업 열기.
            // intent-exempt: `ui_state.popups` 는 settings 윈도우 내부의 별도 PopupManager.
            // host Intent 큐(AppState.popups) 와 별개 — sub-modal 내부 lifecycle 이므로
            // 직접 호출 유지.
            if ui_state.pending_binding.is_some() && !ui_state.popups.is_open("keybinding_conflict")
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
                // intent-exempt: settings 윈도우 내부 sub-modal close (위 주석 참조).
                ui_state.popups.close("keybinding_conflict");
            }
            if ui_state.conflict_cancelled {
                ui_state.conflict_cancelled = false;
                ui_state.pending_binding = None;
                // intent-exempt: settings 윈도우 내부 sub-modal close.
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
                if id == "keybinding_conflict"
                    && let Some(pending) = &pending
                {
                    let conflict_label_raw = crate::settings::KeybindingSettings::label_key_for(
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
