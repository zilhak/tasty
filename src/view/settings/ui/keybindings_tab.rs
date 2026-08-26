use tasty_type_geometry::length::LogicalPx;

use crate::i18n::t;
use crate::plugin::registry_state::ShortcutOverride;
use crate::settings::Settings;
use crate::settings_ui::PluginShortcutSnapshot;

/// 키바인딩 탭 전 서브탭(`entries`/`quick_switch`/`entries_scripts`)이 공유하는
/// 좌측 라벨 컬럼 고정폭. `remote_transfer.rs`(`LABEL_COL_WIDTH`)의 150px을
/// 시작점으로 삼되, en/ko/ja 3개 언어 전체 라벨을 실제 프로덕션 egui 폰트
/// 스택(`TextStyle::Body` = `Theme::font_size_body` 13.0px + CJK fallback, `label_width.rs`
/// 참고)으로 실측한 결과 그대로 쓰면 잘리는 라벨이 있어 올렸다. 최장 실측치는
/// ja `screenshot_to_clipboard_label`("スクリーンショットをクリップボードへ:")의
/// 255.28px((?) 아이콘 슬롯 18px 포함) — 여기에 여유를 두고 4px 그리드에 맞춰
/// 288로 고정한다. 이 실측치는 `label_width.rs`의 `labels_fit_within_fixed_column`
/// 테스트로 항상 재현·재확인 가능하다(라벨 추가/번역 변경 시 실패해 알려준다).
/// 서브탭마다 최장 라벨의 실측 폭이 달라 컬럼 폭이 제각각이던 문제를
/// 이 상수로 통일한다. 4px 그리드 밖 화면 전용 고정 치수 — 대응 Theme 필드
/// 없음(theme.md 참고).
pub(super) const LABEL_COL_WIDTH: LogicalPx = LogicalPx(288.0);

/// 녹화 완료 시 발견된 단축키 충돌의 확인 대기 상태.
#[derive(Debug, Clone)]
pub struct PendingBinding {
    pub target_field: String,
    /// 교체할 (또는 새로 추가할) 대상 인덱스. len()이면 새 추가.
    pub target_idx: usize,
    pub combo: String,
    pub conflicting_field: String,
    pub conflicting_idx: usize,
    // ── quick-switch bare-key 확장 ─────────────────────────────────────
    /// `Some` 이면 이 충돌의 타겟이 quick-switch bare-key 슬롯이다. accept 시
    /// `combo`(합성 표시용) 대신 [`Self::bare_raw_key`] 를 accessor 로 기록한다.
    pub bare_target: Option<BareTarget>,
    /// `bare_target` 이 `Some` 일 때 슬롯에 기록할 raw 키(modifier 없음).
    pub bare_raw_key: String,
    /// 충돌 상대가 **다른 quick-switch 슬롯**이면 `Some`. accept 시 그 슬롯을 비운다
    /// (`conflicting_field`/`conflicting_idx` 는 일반 콤보 필드 전용이므로 슬롯 충돌엔
    /// 쓸 수 없다).
    pub conflicting_bare: Option<BareTarget>,
    /// 팝업에 표시할 충돌 대상 라벨(이미 번역·정리된 문자열). `Some` 이면 팝업이
    /// `label_key_for` 경로 대신 이 값을 그대로 쓴다(슬롯 충돌은 일반 필드 라벨 맵에
    /// 없으므로 필요).
    pub conflicting_label: Option<String>,
}

/// quick-switch bare-key 슬롯의 대상 식별자. modifier 는 `tab_switch_modifier` /
/// `workspace_switch_modifier` 에서 조합되고, 여기서는 raw 키가 어느 슬롯에
/// 속하는지만 나타낸다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BareTarget {
    /// 탭 quick-switch 슬롯 `idx`(0~9 → 표시 "1번"~"10번").
    TabSlot(usize),
    /// 워크스페이스 quick-switch 슬롯 `idx`(0~8 → 표시 "1번"~"9번").
    WorkspaceSlot(usize),
    /// 카테고리 quick-switch 슬롯 `idx`(0~9 → 표시 "1번"~"10번", reserved normal=1).
    CategorySlot(usize),
    TabNext,
    TabPrev,
    WorkspaceNext,
    WorkspacePrev,
    CategoryNext,
    CategoryPrev,
}

/// 녹화 슬롯이 요구하는 캡처 규칙. 일반 콤보 필드는 modifier 필수([`Combo`]),
/// quick-switch 슬롯은 modifier 금지 bare 키([`BareKey`]) — 단 그 축이 "개별 지정"
/// (`KeybindingSettings::INDIVIDUAL_SWITCH_MODIFIER`) 이면 [`IndividualSlot`] 로
/// modifier 포함 자유 콤보를 녹화한다(일반 콤보 필드와 동일 캡처 규칙, 저장 위치만
/// quick-switch 슬롯 필드).
///
/// [`Combo`]: FieldKind::Combo
/// [`BareKey`]: FieldKind::BareKey
/// [`IndividualSlot`]: FieldKind::IndividualSlot
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    Combo,
    BareKey(BareTarget),
    IndividualSlot(BareTarget),
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
    /// 이 슬롯의 캡처 규칙. quick-switch 슬롯이면 `BareKey`(modifier 금지).
    pub field_kind: FieldKind,
}

/// Result of key capture attempt.
#[derive(Debug, PartialEq)]
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
                &settings.general,
                recording_field,
                pending_binding,
                &captured,
                &[
                    (
                        "toggle_settings",
                        "settings.keybindings.toggle_settings_label",
                        None,
                    ),
                    (
                        "toggle_notifications",
                        "settings.keybindings.toggle_notifications_label",
                        None,
                    ),
                    (
                        "toggle_dag_list",
                        "settings.keybindings.toggle_dag_list_label",
                        None,
                    ),
                    (
                        "fullscreen_stage_exit",
                        "settings.keybindings.fullscreen_stage_exit_label",
                        Some("settings.keybindings.fullscreen_stage_exit_desc"),
                    ),
                    (
                        "restore_closed",
                        "settings.keybindings.restore_closed_label",
                        None,
                    ),
                    ("new_window", "settings.keybindings.new_window_label", None),
                    (
                        "quit",
                        "settings.keybindings.quit_label",
                        Some("settings.keybindings.quit_desc"),
                    ),
                    (
                        "quit_immediate",
                        "settings.keybindings.quit_immediate_label",
                        None,
                    ),
                    (
                        "quit_minimize",
                        "settings.keybindings.quit_minimize_label",
                        None,
                    ),
                    (
                        "minimize_window",
                        "settings.keybindings.minimize_window_label",
                        None,
                    ),
                    (
                        "maximize_window",
                        "settings.keybindings.maximize_window_label",
                        None,
                    ),
                    (
                        "close_window",
                        "settings.keybindings.close_window_label",
                        None,
                    ),
                ],
            );
        }
        KeybindingsSubTab::Workspace => {
            draw_keybinding_entries(
                ui,
                &mut settings.keybindings,
                &settings.general,
                recording_field,
                pending_binding,
                &captured,
                &[
                    (
                        "new_workspace",
                        "settings.keybindings.new_workspace_label",
                        None,
                    ),
                    (
                        "rename_workspace",
                        "settings.keybindings.rename_workspace_label",
                        None,
                    ),
                    (
                        "rename_workspace_subtitle",
                        "settings.keybindings.rename_workspace_subtitle_label",
                        None,
                    ),
                    (
                        "close_workspace",
                        "settings.keybindings.close_workspace_label",
                        None,
                    ),
                ],
            );

            vspace(ui, th.spacing_sm);
            ui.separator();
            vspace(ui, th.spacing_xs);

            draw_quick_switch_section(
                ui,
                &mut settings.keybindings,
                &settings.general,
                recording_field,
                pending_binding,
                &captured,
                QuickSwitchKind::Workspace,
            );

            vspace(ui, th.spacing_sm);
            ui.separator();
            vspace(ui, th.spacing_xs);

            draw_quick_switch_section(
                ui,
                &mut settings.keybindings,
                &settings.general,
                recording_field,
                pending_binding,
                &captured,
                QuickSwitchKind::Category,
            );
        }
        KeybindingsSubTab::Pane => {
            draw_keybinding_entries(
                ui,
                &mut settings.keybindings,
                &settings.general,
                recording_field,
                pending_binding,
                &captured,
                &[
                    (
                        "split_pane_vertical",
                        "settings.keybindings.split_pane_vertical_label",
                        None,
                    ),
                    (
                        "split_pane_horizontal",
                        "settings.keybindings.split_pane_horizontal_label",
                        None,
                    ),
                    (
                        "focus_pane_next",
                        "settings.keybindings.focus_pane_next_label",
                        None,
                    ),
                    (
                        "focus_pane_prev",
                        "settings.keybindings.focus_pane_prev_label",
                        None,
                    ),
                    ("close_pane", "settings.keybindings.close_pane_label", None),
                ],
            );
        }
        KeybindingsSubTab::Tab => {
            draw_keybinding_entries(
                ui,
                &mut settings.keybindings,
                &settings.general,
                recording_field,
                pending_binding,
                &captured,
                &[
                    ("new_tab", "settings.keybindings.new_tab_label", None),
                    (
                        "open_markdown",
                        "settings.keybindings.open_markdown_label",
                        None,
                    ),
                    ("next_tab", "settings.keybindings.next_tab_label", None),
                    ("prev_tab", "settings.keybindings.prev_tab_label", None),
                    ("rename_tab", "settings.keybindings.rename_tab_label", None),
                    (
                        "close_active",
                        "settings.keybindings.close_active_label",
                        Some("settings.keybindings.close_active_desc"),
                    ),
                ],
            );

            vspace(ui, th.spacing_sm);
            ui.separator();
            vspace(ui, th.spacing_xs);

            draw_quick_switch_section(
                ui,
                &mut settings.keybindings,
                &settings.general,
                recording_field,
                pending_binding,
                &captured,
                QuickSwitchKind::Tab,
            );
        }
        KeybindingsSubTab::Surface => {
            draw_keybinding_entries(
                ui,
                &mut settings.keybindings,
                &settings.general,
                recording_field,
                pending_binding,
                &captured,
                &[
                    (
                        "split_surface_vertical",
                        "settings.keybindings.split_surface_vertical_label",
                        None,
                    ),
                    (
                        "split_surface_horizontal",
                        "settings.keybindings.split_surface_horizontal_label",
                        None,
                    ),
                    (
                        "focus_surface_next",
                        "settings.keybindings.focus_surface_next_label",
                        None,
                    ),
                    (
                        "focus_surface_prev",
                        "settings.keybindings.focus_surface_prev_label",
                        None,
                    ),
                    (
                        "convert_surface",
                        "settings.keybindings.convert_surface_label",
                        None,
                    ),
                    (
                        "convert_to_markdown",
                        "settings.keybindings.convert_to_markdown_label",
                        None,
                    ),
                    (
                        "close_surface",
                        "settings.keybindings.close_surface_label",
                        None,
                    ),
                ],
            );
        }
        KeybindingsSubTab::Clipboard => {
            draw_keybinding_entries(
                ui,
                &mut settings.keybindings,
                &settings.general,
                recording_field,
                pending_binding,
                &captured,
                &[
                    ("copy", "settings.keybindings.copy_label", None),
                    ("copy_path", "settings.keybindings.copy_path_label", None),
                    ("cut", "settings.keybindings.cut_label", None),
                    ("select_all", "settings.keybindings.select_all_label", None),
                    ("paste", "settings.keybindings.paste_label", None),
                    (
                        "screenshot_to_clipboard",
                        "settings.keybindings.screenshot_to_clipboard_label",
                        None,
                    ),
                ],
            );
        }
        KeybindingsSubTab::Zoom => {
            draw_keybinding_entries(
                ui,
                &mut settings.keybindings,
                &settings.general,
                recording_field,
                pending_binding,
                &captured,
                &[
                    ("zoom_in", "settings.keybindings.zoom_in_label", None),
                    ("zoom_out", "settings.keybindings.zoom_out_label", None),
                    ("zoom_reset", "settings.keybindings.zoom_reset_label", None),
                ],
            );
        }
        KeybindingsSubTab::Image => {
            draw_keybinding_entries(
                ui,
                &mut settings.keybindings,
                &settings.general,
                recording_field,
                pending_binding,
                &captured,
                &[
                    ("image_undo", "settings.keybindings.image_undo_label", None),
                    ("image_redo", "settings.keybindings.image_redo_label", None),
                ],
            );
        }
        KeybindingsSubTab::Explorer => {
            draw_keybinding_entries(
                ui,
                &mut settings.keybindings,
                &settings.general,
                recording_field,
                pending_binding,
                &captured,
                &[
                    (
                        "explorer_refresh",
                        "settings.keybindings.explorer_refresh_label",
                        None,
                    ),
                    (
                        "explorer_go_up",
                        "settings.keybindings.explorer_go_up_label",
                        None,
                    ),
                ],
            );
        }
        KeybindingsSubTab::Scripts => {
            draw_script_bindings(ui, settings, recording_field, &captured);
        }
        KeybindingsSubTab::Preset => {
            draw_preset_subtab(
                ui,
                &mut settings.keybindings,
                &settings.general,
                selected_preset,
            );
        }
        KeybindingsSubTab::Plugins => {
            draw_plugins_subtab(
                ui,
                plugin_shortcuts,
                plugin_shortcuts_selected,
                plugin_shortcuts_draft,
                &settings.keybindings,
                &settings.general,
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

/// Preset 서브탭: 좌측 프리셋 목록, 우측 미리보기 테이블 + 적용 버튼.
mod capture;
mod entries;
mod entries_scripts;
#[cfg(test)]
mod label_width;
mod plugins;
mod preset;
mod quick_switch;

pub use capture::{capture_bare_key, capture_winit_key_combo};
use entries::draw_keybinding_entries;
use entries_scripts::draw_script_bindings;
use plugins::draw_plugins_subtab;
use preset::draw_preset_subtab;
use quick_switch::{QuickSwitchKind, draw_quick_switch_section};
pub use quick_switch::{clear_bare_target, set_bare_target};
use tasty_ui_widgets::vspace;
