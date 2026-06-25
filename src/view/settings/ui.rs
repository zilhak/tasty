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
use tasty_host_plugin::SettingsPageEntry;

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
///
/// Host-internal variants (`Theme` / `General` / `Display` / `Terminal`) are
/// hardcoded; plugin-contributed pages appear as
/// `Plugin { plugin_id, page_id }` and are resolved against
/// `SettingsUiState::settings_pages` at render time. `page_id` 단독으로는
/// 서로 다른 plugin 이 동일 id 를 contribute 할 경우 충돌하므로
/// `(plugin_id, page_id)` 복합키로 식별한다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AppearanceSubTab {
    Theme,
    /// 현재 프리셋의 색을 개별 override 하는 picker (theme_overrides 편집).
    Colors,
    General,
    /// UI scale (sm/md/lg) 전용 섹션.
    Display,
    /// app-chrome 테마 (accent / sidebar bg / active tab indicator) 전용 섹션.
    Tasty,
    Terminal,
    /// Plugin-contributed sub-tab. 복합키:
    /// - `plugin_id` = `SettingsPageEntry::plugin_id`
    /// - `page_id` = `SettingsPageContribute::id` (plugin scope 내)
    Plugin {
        plugin_id: String,
        page_id: String,
    },
}

/// Sub-tab within the Plugin tab. Plugin-contributed pages keyed by
/// `(plugin_id, page_id)` — 동일 `page_id` 를 contribute 한 두 plugin 이 충돌하지
/// 않도록 복합키 사용.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PluginSubTab {
    Plugin { plugin_id: String, page_id: String },
}

/// L2 section within the General L1 tab.
///
/// 디자인 General L2 = General / Notifications / Accessibility.
/// (Clipboard 는 플러그인 기능이라 네이티브 설정에서 제외, Updates 는 Misc 로 이동.)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GeneralSubTab {
    General,
    Notifications,
    Accessibility,
}

/// L2 section within the Terminal L1 tab.
///
/// 디자인 Terminal L2 = General(터미널 동작 설정) / Performance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TerminalSubTab {
    General,
    Performance,
}

/// L2 section within the Misc L1 tab.
///
/// 디자인 Misc L2 = Tastyrc(Windows 전용). 비-Windows 에서는 L2 항목이 없어
/// Misc 가 빈 탭(empty-state)이 된다. `Tastyrc` 는 Windows 전용 (tasty 빌트인
/// bashrc 편집) — 비-Windows 에서는 dead variant 가 되지만 exhaustive match
/// 안전성을 위해 variant 자체는 유지하고 `allow(dead_code)` 로 경고만 억제한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MiscSubTab {
    #[cfg_attr(not(windows), allow(dead_code))]
    Tastyrc,
}

/// Active L1 tab in the settings window. 디자인 2-level IA 의 상단 7탭.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SettingsTab {
    General,
    Terminal,
    Appearance,
    Keybindings,
    FileHandler,
    Misc,
    Plugins,
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
    /// Active sub-tab within plugin tab. `None` = 등록된 plugin page 가 없거나
    /// 사용자가 아직 어떤 sub-tab 도 선택하지 않은 상태.
    pub(crate) plugin_sub_tab: Option<PluginSubTab>,
    /// Active L2 section within the General L1 tab.
    general_sub_tab: GeneralSubTab,
    /// Active L2 section within the Terminal L1 tab.
    terminal_sub_tab: TerminalSubTab,
    /// Active L2 section within the Misc L1 tab. 비-Windows 에서는 live variant 가
    /// 없어 empty-state 가 그려지며 이 값은 사용되지 않는다.
    misc_sub_tab: MiscSubTab,
    /// L2 사이드바 섹션 필터 텍스트. L1 전환 시 클리어. 7개 L1 탭이 공유한다
    /// (디자인은 한 번에 하나의 L1 만 보이므로 단일 필드로 충분).
    l2_filter: String,
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
    /// Plugin 이 contribute 한 settings page 들의 스냅샷. 모달 오픈 시
    /// host 의 `PluginManager::settings_pages` 에서 복사. 외관 탭의 sub-tab
    /// 합성과 plugin page 렌더링에서 참조한다. 비어 있으면 plugin sub-tab
    /// 자체가 표시되지 않는다 (= dead-setting 미노출 정책).
    pub settings_pages: Vec<SettingsPageEntry>,
}

impl SettingsUiState {
    /// 단축키 녹화 중인지 여부.
    pub fn is_recording(&self) -> bool {
        self.recording_field.is_some()
    }

    /// Plugins 모달의 `Configure` 진입점에서 호출 — 첫 진입 탭을 `Plugin` 으로 설정.
    pub fn select_plugin_tab(&mut self) {
        self.active_tab = SettingsTab::Plugins;
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
            plugin_sub_tab: None,
            general_sub_tab: GeneralSubTab::General,
            terminal_sub_tab: TerminalSubTab::General,
            misc_sub_tab: MiscSubTab::Tastyrc,
            l2_filter: String::new(),
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
            settings_pages: Vec::new(),
        }
    }

    /// Plugin 이 contribute 한 settings page 스냅샷을 주입한다. 모달 오픈 직전에
    /// host App 이 호출한다. 빈 vec 으로 호출하면 plugin sub-tab 이 사라진다.
    pub fn set_settings_pages(&mut self, pages: Vec<SettingsPageEntry>) {
        self.settings_pages = pages;
    }
}

/// [`draw_settings_panel`] 에 전달되는 렌더 컨텍스트 묶음.
///
/// settings modal 진입점의 인자가 시기별로 누적되어 clippy `too_many_arguments`
/// 임계치를 넘어 struct 로 묶음 — 이후 인자 추가 시 시그니처 변경 없이 필드만 늘린다.
pub struct SettingsPanelCtx<'a> {
    pub settings: &'a mut Settings,
    pub ui_state: &'a mut SettingsUiState,
    pub captured_double_tap: &'a mut Option<String>,
    pub file_format: &'a FileFormatRegistry,
    pub file_handler: &'a FileHandlerRegistry,
    pub user_config_path: Option<&'a std::path::Path>,
}

/// Draw settings directly as a full-window panel (for modal windows).
/// Returns true if Save was clicked, false if Cancel was clicked, None otherwise.
pub fn draw_settings_panel(ctx: &egui::Context, panel: SettingsPanelCtx<'_>) -> Option<bool> {
    let SettingsPanelCtx {
        settings,
        ui_state,
        captured_double_tap,
        file_format,
        file_handler,
        user_config_path,
    } = panel;
    if ui_state.draft.is_none() {
        ui_state.draft = Some(settings.clone());
    }

    // Lazily load system font list on first access
    if ui_state.font_families.is_none() {
        let font_config = crate::font::FontConfig::new(14.0, "");
        ui_state.font_families = Some(font_config.list_families());
    }

    // Lazily load ~/.tasty/bashrc.user on first settings open.
    // tasty 빌트인 편집(Misc 탭)은 Windows 전용이므로 비-Windows 에선 로드하지 않는다.
    #[cfg(windows)]
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
                    // tasty 빌트인 bashrc 편집은 Windows 전용 (Misc 탭). 비-Windows 는
                    // draft 가 비어 있고 저장할 것도 없다.
                    #[cfg(windows)]
                    if let Some(bashrc) = &ui_state.bashrc_user_draft {
                        crate::settings::general::save_user_bashrc(bashrc);
                    }
                    // 선택된 테마 즉시 적용 — 설정 화면에서 콤보 선택은 이미 apply_theme 으로
                    // base/overrides 를 갱신했고, 여기서는 전역 Theme 인스턴스만 install.
                    // host UI zoom 을 항상 실어야 배율이 1.0 으로 리셋되지 않는다.
                    let ui_zoom = settings.appearance.ui_scale_factor();
                    tasty_themes::install_global_with_zoom(&settings.appearance, ui_zoom);
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
            // L1 — 디자인 2-level IA 의 상단 7탭. 나머지 섹션은 각 L1 의 좌측
            // L2 사이드바로 들어간다.
            let tabs = vec![
                (SettingsTab::General, t("settings.tab.general")),
                (SettingsTab::Terminal, t("settings.tab.terminal")),
                (SettingsTab::Appearance, t("settings.tab.appearance")),
                (SettingsTab::Keybindings, t("settings.tab.keybindings")),
                (SettingsTab::FileHandler, t("settings.tab.file_handler")),
                (SettingsTab::Misc, t("settings.tab.misc")),
                (SettingsTab::Plugins, t("settings.tab.plugin")),
            ];

            let prev_tab = ui_state.active_tab;
            tasty_ui_widgets::horizontal_tab_bar_with_arrows(
                ui,
                "settings_l1_tabs",
                &tabs,
                &mut ui_state.active_tab,
            );
            // L1 전환 시 L2 필터를 초기화 (디자인: pickL1 → setFilter("")).
            if ui_state.active_tab != prev_tab {
                ui_state.l2_filter.clear();
            }
        });
        ui.separator();

        {
            let mut draft = ui_state.draft.take().unwrap();
            let active_tab = ui_state.active_tab;

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .drag_to_scroll(false)
                .show(ui, |ui| {
                    tasty_ui_widgets::tab_content_frame(ui, |ui| match active_tab {
                        SettingsTab::General => {
                            draw_general_group(ui, &mut draft, ui_state)
                        }
                        SettingsTab::Terminal => {
                            draw_terminal_group(ui, &mut draft, ui_state)
                        }
                        SettingsTab::Appearance => draw_appearance_tab(
                            ui,
                            &mut draft,
                            &mut ui_state.appearance_sub_tab,
                            &mut ui_state.font_families,
                            &mut ui_state.font_filter,
                            &mut ui_state.preview_font_loaded,
                            &ui_state.settings_pages,
                            &mut ui_state.l2_filter,
                        ),
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
                            &mut ui_state.l2_filter,
                        ),
                        SettingsTab::FileHandler => draw_file_handler_tab(
                            ui,
                            &mut ui_state.file_handler_sub_tab,
                            &mut ui_state.extension_priority_draft,
                            &mut ui_state.extension_priority_new_input,
                            &mut ui_state.fh_edit_draft,
                            file_format,
                            file_handler,
                            &mut ui_state.l2_filter,
                        ),
                        SettingsTab::Misc => draw_misc_group(ui, ui_state),
                        SettingsTab::Plugins => draw_plugin_tab(
                            ui,
                            &mut draft,
                            &mut ui_state.plugin_sub_tab,
                            &mut ui_state.font_families,
                            &mut ui_state.font_filter,
                            &mut ui_state.preview_font_loaded,
                            &ui_state.settings_pages,
                            &mut ui_state.l2_filter,
                        ),
                    });
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

/// General L1 탭: 좌측 L2 사이드바(필터 포함) + 우측 섹션 콘텐츠.
///
/// 디자인 General L2 = General / Notifications / Accessibility.
fn draw_general_group(ui: &mut egui::Ui, draft: &mut Settings, ui_state: &mut SettingsUiState) {
    let th = crate::theme::theme();
    ui.add_space(8.0);

    let available_height = ui.available_height() - 8.0 - 14.0;

    let sections: Vec<(GeneralSubTab, String)> = vec![
        (
            GeneralSubTab::General,
            t("settings.tab.general").to_string(),
        ),
        (
            GeneralSubTab::Notifications,
            t("settings.tab.notifications").to_string(),
        ),
        (
            GeneralSubTab::Accessibility,
            t("settings.misc.subtab.accessibility").to_string(),
        ),
    ];

    let current = ui_state.general_sub_tab;
    let mut selected_new: Option<GeneralSubTab> = None;
    let filter_lc = ui_state.l2_filter.to_lowercase();
    tasty_ui_widgets::two_depth_layout_filtered(
        ui,
        &th,
        available_height,
        &mut ui_state.l2_filter,
        t("settings.filter.sections"),
        |ui| {
            let mut any = false;
            for (tab, label) in &sections {
                if !filter_lc.is_empty() && !label.to_lowercase().contains(&filter_lc) {
                    continue;
                }
                any = true;
                let selected = current == *tab;
                if ui.selectable_label(selected, label.as_str()).clicked() {
                    selected_new = Some(*tab);
                }
            }
            if !any {
                ui.label(egui::RichText::new(t("settings.filter.no_matches")).color(th.subtext0));
            }
        },
        |ui| match current {
            GeneralSubTab::General => draw_general_tab(ui, draft),
            GeneralSubTab::Notifications => draw_notifications_tab(ui, draft),
            GeneralSubTab::Accessibility => draw_accessibility_tab(ui, draft),
        },
    );
    if let Some(new) = selected_new {
        ui_state.general_sub_tab = new;
    }
}

/// Terminal L1 탭: 좌측 L2 사이드바(필터 포함) + 우측 섹션 콘텐츠.
///
/// 디자인 Terminal L2 = General(터미널 동작 설정) / Performance.
fn draw_terminal_group(ui: &mut egui::Ui, draft: &mut Settings, ui_state: &mut SettingsUiState) {
    let th = crate::theme::theme();
    ui.add_space(8.0);

    let available_height = ui.available_height() - 8.0 - 14.0;

    let sections: Vec<(TerminalSubTab, String)> = vec![
        (
            TerminalSubTab::General,
            t("settings.tab.general").to_string(),
        ),
        (
            TerminalSubTab::Performance,
            t("settings.misc.subtab.performance").to_string(),
        ),
    ];

    let current = ui_state.terminal_sub_tab;
    let mut selected_new: Option<TerminalSubTab> = None;
    let filter_lc = ui_state.l2_filter.to_lowercase();
    tasty_ui_widgets::two_depth_layout_filtered(
        ui,
        &th,
        available_height,
        &mut ui_state.l2_filter,
        t("settings.filter.sections"),
        |ui| {
            let mut any = false;
            for (tab, label) in &sections {
                if !filter_lc.is_empty() && !label.to_lowercase().contains(&filter_lc) {
                    continue;
                }
                any = true;
                let selected = current == *tab;
                if ui.selectable_label(selected, label.as_str()).clicked() {
                    selected_new = Some(*tab);
                }
            }
            if !any {
                ui.label(egui::RichText::new(t("settings.filter.no_matches")).color(th.subtext0));
            }
        },
        |ui| match current {
            TerminalSubTab::General => draw_terminal_tab(ui, draft),
            TerminalSubTab::Performance => draw_performance_tab(ui, draft),
        },
    );
    if let Some(new) = selected_new {
        ui_state.terminal_sub_tab = new;
    }
}

/// Misc L1 탭: 좌측 L2 사이드바(필터 포함) + 우측 섹션 콘텐츠.
///
/// 디자인 Misc L2 = Tastyrc(Windows 전용). 비-Windows 에서는 L2 항목이 없어
/// 사이드바가 비고 콘텐츠 영역에 empty-state 를 그린다.
fn draw_misc_group(ui: &mut egui::Ui, ui_state: &mut SettingsUiState) {
    let th = crate::theme::theme();
    ui.add_space(8.0);

    let available_height = ui.available_height() - 8.0 - 14.0;

    // Windows 에서만 Tastyrc 섹션을 push 한다. 비-Windows 에선 L2 가 없다.
    #[cfg_attr(not(windows), allow(unused_mut))]
    let mut sections: Vec<(MiscSubTab, String)> = Vec::new();
    #[cfg(windows)]
    sections.push((
        MiscSubTab::Tastyrc,
        t("settings.misc.subtab.tastyrc").to_string(),
    ));

    let current = ui_state.misc_sub_tab;
    let mut selected_new: Option<MiscSubTab> = None;
    let filter_lc = ui_state.l2_filter.to_lowercase();
    tasty_ui_widgets::two_depth_layout_filtered(
        ui,
        &th,
        available_height,
        &mut ui_state.l2_filter,
        t("settings.filter.sections"),
        |ui| {
            let mut any = false;
            for (tab, label) in &sections {
                if !filter_lc.is_empty() && !label.to_lowercase().contains(&filter_lc) {
                    continue;
                }
                any = true;
                let selected = current == *tab;
                if ui.selectable_label(selected, label.as_str()).clicked() {
                    selected_new = Some(*tab);
                }
            }
            // 필터로 0건이 된 경우에만 안내. 섹션 자체가 없는(비-Windows) 경우는
            // 우측 콘텐츠의 empty-state 가 설명을 대신하므로 사이드바는 비워둔다.
            if !any && !filter_lc.is_empty() {
                ui.label(egui::RichText::new(t("settings.filter.no_matches")).color(th.subtext0));
            }
        },
        |ui| match current {
            #[cfg(windows)]
            MiscSubTab::Tastyrc => draw_tastyrc_subtab(ui, &mut ui_state.bashrc_user_draft),
            #[cfg(not(windows))]
            MiscSubTab::Tastyrc => {
                ui.vertical_centered(|ui| {
                    ui.add_space(24.0);
                    ui.label(
                        egui::RichText::new(t("settings.misc.empty")).color(th.subtext0),
                    );
                });
            }
        },
    );
    if let Some(new) = selected_new {
        ui_state.misc_sub_tab = new;
    }
}
