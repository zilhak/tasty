use crate::i18n::t;
use crate::plugin::registry_state::ShortcutOverride;
use crate::settings::Settings;
use crate::settings_ui::PluginShortcutSnapshot;

/// 녹화 완료 시 발견된 단축키 충돌의 확인 대기 상태.
#[derive(Debug, Clone)]
pub struct PendingBinding {
    pub target_field: String,
    /// 교체할 (또는 새로 추가할) 대상 인덱스. len()이면 새 추가.
    pub target_idx: usize,
    pub combo: String,
    pub conflicting_field: String,
    pub conflicting_idx: usize,
}

/// Sub-tab within the Keybindings tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeybindingsSubTab {
    General,
    Workspace,
    Pane,
    Tab,
    Surface,
    Clipboard,
    Zoom,
    Image,
    Explorer,
    Scripts,
    Preset,
    Plugins,
}

/// 녹화 중인 필드 식별자 — 어떤 필드의 어느 슬롯을 기록 중인지.
#[derive(Debug, Clone)]
pub struct RecordingSlot {
    pub field_id: String,
    /// 기존 바인딩 교체 시 인덱스, 새 바인딩 추가 시 `bindings.len()`.
    pub idx: usize,
}

/// Result of key capture attempt.
pub enum KeyCapture {
    /// No key pressed yet.
    None,
    /// User pressed Escape — clear the binding.
    Clear,
    /// A valid key combination was captured.
    Combo(String),
}

/// Keybindings 탭 콘텐츠. L2 사이드바(섹션 목록·필터·선택)는 settings 셸이
/// 소유하므로 여기서는 활성 `sub_tab` 의 바인딩 엔트리만 그린다.
#[allow(clippy::too_many_arguments)]
pub fn draw_keybindings_tab(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    recording_field: &mut Option<RecordingSlot>,
    sub_tab: KeybindingsSubTab,
    selected_preset: &mut Option<String>,
    pending_binding: &mut Option<PendingBinding>,
    captured_double_tap: &mut Option<String>,
    captured_winit_combo: &mut Option<KeyCapture>,
    plugin_shortcuts: &PluginShortcutSnapshot,
    plugin_shortcuts_selected: &mut Option<String>,
    plugin_shortcuts_draft: &mut std::collections::BTreeMap<
        (String, String),
        Option<ShortcutOverride>,
    >,
) {
    let th = crate::theme::theme();
    let current = sub_tab;

    // winit에서 직접 캡처한 키 조합을 사용. double-tap이 우선.
    let captured = if recording_field.is_some() {
        if let Some(dt) = captured_double_tap.take() {
            KeyCapture::Combo(dt)
        } else {
            captured_winit_combo.take().unwrap_or(KeyCapture::None)
        }
    } else {
        KeyCapture::None
    };

    match current {
        KeybindingsSubTab::General => {
            draw_keybinding_entries(
                ui,
                &mut settings.keybindings,
                recording_field,
                pending_binding,
                &captured,
                &[
                    (
                        "toggle_settings",
                        "settings.keybindings.toggle_settings_label",
                    ),
                    (
                        "toggle_notifications",
                        "settings.keybindings.toggle_notifications_label",
                    ),
                    (
                        "toggle_clipboard_viewer",
                        "settings.keybindings.toggle_clipboard_viewer_label",
                    ),
                    (
                        "restore_closed",
                        "settings.keybindings.restore_closed_label",
                    ),
                    ("new_window", "settings.keybindings.new_window_label"),
                    ("quit", "settings.keybindings.quit_label"),
                    (
                        "quit_immediate",
                        "settings.keybindings.quit_immediate_label",
                    ),
                    ("quit_minimize", "settings.keybindings.quit_minimize_label"),
                    (
                        "minimize_window",
                        "settings.keybindings.minimize_window_label",
                    ),
                    (
                        "maximize_window",
                        "settings.keybindings.maximize_window_label",
                    ),
                    ("close_window", "settings.keybindings.close_window_label"),
                ],
            );
        }
        KeybindingsSubTab::Workspace => {
            draw_keybinding_entries(
                ui,
                &mut settings.keybindings,
                recording_field,
                pending_binding,
                &captured,
                &[
                    ("new_workspace", "settings.keybindings.new_workspace_label"),
                    (
                        "rename_workspace",
                        "settings.keybindings.rename_workspace_label",
                    ),
                    (
                        "rename_workspace_subtitle",
                        "settings.keybindings.rename_workspace_subtitle_label",
                    ),
                    (
                        "close_workspace",
                        "settings.keybindings.close_workspace_label",
                    ),
                ],
            );

            vspace(ui, th.spacing_sm);
            ui.separator();
            vspace(ui, th.spacing_xs);

            egui::Grid::new("ws_modifier_grid")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    ui.label(t("settings.keybindings.workspace_switch_modifier_label"));
                    egui::ComboBox::from_id_salt("workspace_switch_modifier")
                        .selected_text(modifier_display(
                            &settings.keybindings.workspace_switch_modifier,
                        ))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut settings.keybindings.workspace_switch_modifier,
                                "ctrl".to_string(),
                                "Ctrl",
                            );
                            ui.selectable_value(
                                &mut settings.keybindings.workspace_switch_modifier,
                                "alt".to_string(),
                                "Alt",
                            );
                        });
                    ui.end_row();
                });
        }
        KeybindingsSubTab::Pane => {
            draw_keybinding_entries(
                ui,
                &mut settings.keybindings,
                recording_field,
                pending_binding,
                &captured,
                &[
                    (
                        "split_pane_vertical",
                        "settings.keybindings.split_pane_vertical_label",
                    ),
                    (
                        "split_pane_horizontal",
                        "settings.keybindings.split_pane_horizontal_label",
                    ),
                    (
                        "focus_pane_next",
                        "settings.keybindings.focus_pane_next_label",
                    ),
                    (
                        "focus_pane_prev",
                        "settings.keybindings.focus_pane_prev_label",
                    ),
                    ("close_pane", "settings.keybindings.close_pane_label"),
                ],
            );
        }
        KeybindingsSubTab::Tab => {
            draw_keybinding_entries(
                ui,
                &mut settings.keybindings,
                recording_field,
                pending_binding,
                &captured,
                &[
                    ("new_tab", "settings.keybindings.new_tab_label"),
                    ("open_markdown", "settings.keybindings.open_markdown_label"),
                    ("next_tab", "settings.keybindings.next_tab_label"),
                    ("prev_tab", "settings.keybindings.prev_tab_label"),
                    ("rename_tab", "settings.keybindings.rename_tab_label"),
                    ("close_active", "settings.keybindings.close_active_label"),
                ],
            );

            vspace(ui, th.spacing_sm);
            ui.separator();
            vspace(ui, th.spacing_xs);

            egui::Grid::new("tab_modifier_grid")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    ui.label(t("settings.keybindings.tab_switch_modifier_label"));
                    egui::ComboBox::from_id_salt("tab_switch_modifier")
                        .selected_text(modifier_display(&settings.keybindings.tab_switch_modifier))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut settings.keybindings.tab_switch_modifier,
                                "ctrl".to_string(),
                                "Ctrl",
                            );
                            ui.selectable_value(
                                &mut settings.keybindings.tab_switch_modifier,
                                "alt".to_string(),
                                "Alt",
                            );
                        });
                    ui.end_row();
                });
        }
        KeybindingsSubTab::Surface => {
            draw_keybinding_entries(
                ui,
                &mut settings.keybindings,
                recording_field,
                pending_binding,
                &captured,
                &[
                    (
                        "split_surface_vertical",
                        "settings.keybindings.split_surface_vertical_label",
                    ),
                    (
                        "split_surface_horizontal",
                        "settings.keybindings.split_surface_horizontal_label",
                    ),
                    (
                        "focus_surface_next",
                        "settings.keybindings.focus_surface_next_label",
                    ),
                    (
                        "focus_surface_prev",
                        "settings.keybindings.focus_surface_prev_label",
                    ),
                    (
                        "convert_surface",
                        "settings.keybindings.convert_surface_label",
                    ),
                    (
                        "convert_to_markdown",
                        "settings.keybindings.convert_to_markdown_label",
                    ),
                    ("close_surface", "settings.keybindings.close_surface_label"),
                ],
            );
        }
        KeybindingsSubTab::Clipboard => {
            draw_keybinding_entries(
                ui,
                &mut settings.keybindings,
                recording_field,
                pending_binding,
                &captured,
                &[
                    ("copy", "settings.keybindings.copy_label"),
                    ("copy_path", "settings.keybindings.copy_path_label"),
                    ("cut", "settings.keybindings.cut_label"),
                    ("select_all", "settings.keybindings.select_all_label"),
                    ("paste", "settings.keybindings.paste_label"),
                ],
            );
        }
        KeybindingsSubTab::Zoom => {
            draw_keybinding_entries(
                ui,
                &mut settings.keybindings,
                recording_field,
                pending_binding,
                &captured,
                &[
                    ("zoom_in", "settings.keybindings.zoom_in_label"),
                    ("zoom_out", "settings.keybindings.zoom_out_label"),
                    ("zoom_reset", "settings.keybindings.zoom_reset_label"),
                ],
            );
        }
        KeybindingsSubTab::Image => {
            draw_keybinding_entries(
                ui,
                &mut settings.keybindings,
                recording_field,
                pending_binding,
                &captured,
                &[
                    ("image_undo", "settings.keybindings.image_undo_label"),
                    ("image_redo", "settings.keybindings.image_redo_label"),
                ],
            );
        }
        KeybindingsSubTab::Explorer => {
            draw_keybinding_entries(
                ui,
                &mut settings.keybindings,
                recording_field,
                pending_binding,
                &captured,
                &[
                    (
                        "explorer_refresh",
                        "settings.keybindings.explorer_refresh_label",
                    ),
                    (
                        "explorer_go_up",
                        "settings.keybindings.explorer_go_up_label",
                    ),
                ],
            );
        }
        KeybindingsSubTab::Scripts => {
            draw_script_bindings(ui, settings, recording_field, &captured);
        }
        KeybindingsSubTab::Preset => {
            draw_preset_subtab(ui, &mut settings.keybindings, selected_preset);
        }
        KeybindingsSubTab::Plugins => {
            draw_plugins_subtab(
                ui,
                plugin_shortcuts,
                plugin_shortcuts_selected,
                plugin_shortcuts_draft,
                &settings.keybindings,
            );
        }
    }

    if !matches!(
        current,
        KeybindingsSubTab::Preset | KeybindingsSubTab::Plugins
    ) {
        vspace(ui, th.spacing_sm);
        ui.label(
            egui::RichText::new(t("settings.keybindings.hint_esc_to_clear"))
                .small()
                .color(th.text_disabled()),
        );
    }
}

fn modifier_display(modifier: &str) -> &str {
    match modifier.to_lowercase().as_str() {
        "alt" => "Alt",
        _ => "Ctrl",
    }
}

/// Preset 서브탭: 좌측 프리셋 목록, 우측 미리보기 테이블 + 적용 버튼.
mod capture;
mod entries;
mod entries_scripts;
mod plugins;
mod preset;

pub use capture::capture_winit_key_combo;
use entries::draw_keybinding_entries;
use entries_scripts::draw_script_bindings;
use plugins::draw_plugins_subtab;
use preset::draw_preset_subtab;
use tasty_ui_widgets::vspace;
