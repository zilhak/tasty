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
use tasty_plugin_manifest::SettingsCategory;
use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{Button, ButtonVariant};

/// L2 사이드바 폭. 디자인 `--tasty-settings-sidebar-width` = 200.
const SETTINGS_SIDEBAR_WIDTH: LogicalPx = LogicalPx(200.0);
/// L1 헤더 밴드 높이. 디자인 header `height: 44`.
const SETTINGS_HEADER_HEIGHT: LogicalPx = LogicalPx(44.0);
/// active L1 탭 하단 인디케이터 두께. 디자인 `border-bottom: 2px accent`.
const SETTINGS_TAB_UNDERLINE: LogicalPx = LogicalPx(2.0);
/// L1 탭 사이 간격. 디자인 header `gap: 2`.
const L1_TAB_GAP: LogicalPx = LogicalPx(2.0);
/// 좌측 "Settings" 타이틀 ↔ 탭 구분선 높이. 디자인 jsx:468 `height: 20`.
const SETTINGS_TITLE_DIVIDER_HEIGHT: LogicalPx = LogicalPx(20.0);
/// 구분선 우측 여백. 디자인 jsx:468 `margin: 0 size-14 0 space-sm` 의 size-14.
const SETTINGS_TITLE_DIVIDER_MARGIN_R: LogicalPx = LogicalPx(14.0);
/// 푸터 좌우 패딩. 디자인 footer `padding: space-md size-14` 의 수평값.
const SETTINGS_FOOTER_PAD_X: i8 = 14;

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
    /// Explorer (내장 파일 관리자) surface 전용 폰트 override 섹션 (T11). 과거엔
    /// `com.tasty.explorer` plugin 이 settings page 로 contribute 했으나 host builtin
    /// 승격 후 본체 고정 섹션이 됐다.
    Explorer,
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

    /// debug 전용 — 탭 키 문자열로 `active_tab` 을 설정한다 (`debug.settings.open`).
    /// 알 수 없는 키면 `false` 를 반환하고 탭을 바꾸지 않는다.
    #[cfg(debug_assertions)]
    pub fn select_tab_by_key(&mut self, key: &str) -> bool {
        let tab = match key {
            "general" => SettingsTab::General,
            "terminal" => SettingsTab::Terminal,
            "appearance" => SettingsTab::Appearance,
            "keybindings" => SettingsTab::Keybindings,
            "file_handler" | "file-handler" | "filehandler" => SettingsTab::FileHandler,
            "misc" => SettingsTab::Misc,
            "plugins" => SettingsTab::Plugins,
            _ => return false,
        };
        self.active_tab = tab;
        true
    }

    /// debug 전용 — 현재 활성 L1 탭의 L2 섹션(하위탭)을 키 문자열로 선택한다
    /// (`debug.settings.open` 의 `subtab` 인자). 반드시 [`select_tab_by_key`] 로
    /// L1 을 먼저 정한 뒤 호출한다 — 키는 활성 L1 탭에 종속이다.
    ///
    /// 알 수 없는 키(또는 해당 L1 이 정적 L2 키를 갖지 않는 경우, 예: Plugins 의
    /// 동적 plugin page)면 `false` 를 반환하고 섹션을 바꾸지 않아 L1 기본 L2 가
    /// 유지된다.
    #[cfg(debug_assertions)]
    pub fn select_section_by_key(&mut self, key: &str) -> bool {
        match self.active_tab {
            SettingsTab::General => {
                self.general_sub_tab = match key {
                    "general" => GeneralSubTab::General,
                    "notifications" => GeneralSubTab::Notifications,
                    "accessibility" => GeneralSubTab::Accessibility,
                    _ => return false,
                };
                true
            }
            SettingsTab::Terminal => {
                self.terminal_sub_tab = match key {
                    "general" => TerminalSubTab::General,
                    "performance" => TerminalSubTab::Performance,
                    _ => return false,
                };
                true
            }
            SettingsTab::Appearance => {
                self.appearance_sub_tab = match key {
                    "theme" => AppearanceSubTab::Theme,
                    "colors" => AppearanceSubTab::Colors,
                    "general" => AppearanceSubTab::General,
                    "display" => AppearanceSubTab::Display,
                    "tasty" => AppearanceSubTab::Tasty,
                    "terminal" => AppearanceSubTab::Terminal,
                    "explorer" => AppearanceSubTab::Explorer,
                    _ => return false,
                };
                true
            }
            SettingsTab::Keybindings => {
                self.keybindings_sub_tab = match key {
                    "general" => KeybindingsSubTab::General,
                    "workspace" => KeybindingsSubTab::Workspace,
                    "pane" => KeybindingsSubTab::Pane,
                    "tab" => KeybindingsSubTab::Tab,
                    "surface" => KeybindingsSubTab::Surface,
                    "clipboard" => KeybindingsSubTab::Clipboard,
                    "zoom" => KeybindingsSubTab::Zoom,
                    "image" => KeybindingsSubTab::Image,
                    "explorer" => KeybindingsSubTab::Explorer,
                    "scripts" => KeybindingsSubTab::Scripts,
                    "preset" => KeybindingsSubTab::Preset,
                    "plugins" => KeybindingsSubTab::Plugins,
                    _ => return false,
                };
                true
            }
            SettingsTab::FileHandler => {
                self.file_handler_sub_tab = match key {
                    "extension_mapping" | "extension-mapping" | "extensionmapping" => {
                        FileHandlerSubTab::ExtensionMapping
                    }
                    "detectors" => FileHandlerSubTab::Detectors,
                    "handlers" => FileHandlerSubTab::Handlers,
                    _ => return false,
                };
                true
            }
            SettingsTab::Misc => {
                self.misc_sub_tab = match key {
                    "tastyrc" => MiscSubTab::Tastyrc,
                    _ => return false,
                };
                true
            }
            // Plugins L2 는 plugin 이 동적으로 contribute 한 page (복합키
            // `(plugin_id, page_id)`) 라 정적 키로 주소화하지 않는다.
            SettingsTab::Plugins => false,
        }
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
    let th = crate::theme::theme();
    let sep = egui::Stroke::new(th.border_width.value(), th.surface1.to_egui());

    egui::CentralPanel::default()
        .frame(egui::Frame::NONE.fill(th.bg_panel().to_egui()))
        .show(ctx, |ui| {
            // ── L1 헤더 밴드 (디자인 header height44 / bg-sidebar / border-bottom) ──
            let header = egui::TopBottomPanel::top("settings_header")
                .exact_height(SETTINGS_HEADER_HEIGHT.value())
                .resizable(false)
                .show_separator_line(false)
                .frame(egui::Frame::NONE.fill(th.bg_sidebar().to_egui()))
                .show_inside(ui, |ui| draw_l1_tab_band(ui, &th, ui_state));
            let hr = header.response.rect;
            ui.painter().hline(hr.x_range(), hr.bottom() - 0.5, sep);
            // 밴드 우측 close ✕ → 취소와 동일 경로(draft 폐기 + 모달 닫기).
            if header.inner {
                ui_state.bashrc_user_draft = None;
                ui_state.extension_priority_draft = None;
                ui_state.fh_edit_draft = file_handler_tab::FileHandlerEditDraft::default();
                result = Some(false);
            }

            // ── L2 영속 사이드바 (디자인 width200 / bg-sidebar / border-right) ──
            let sections = build_l2_sections(ui_state);
            let l2_placeholder = if ui_state.active_tab == SettingsTab::Plugins {
                t("settings.filter.plugins")
            } else {
                t("settings.filter.sections")
            };
            let side = egui::SidePanel::left("settings_l2_sidebar")
                .exact_width(SETTINGS_SIDEBAR_WIDTH.value())
                .resizable(false)
                .show_separator_line(false)
                .frame(egui::Frame::NONE.fill(th.bg_sidebar().to_egui()))
                .show_inside(ui, |ui| {
                    draw_l2_sidebar(ui, &th, &sections, &mut ui_state.l2_filter, l2_placeholder)
                });
            let sr = side.response.rect;
            // SidePanel response.rect 은 exact_width 를 넘는 resize handle 영역을
            // 포함하므로 border 는 left + width 에 그린다 (design-parity-notes).
            ui.painter().vline(
                sr.left() + SETTINGS_SIDEBAR_WIDTH.value() - 0.5,
                sr.y_range(),
                sep,
            );
            if let Some(i) = side.inner {
                apply_l2_select(ui_state, &sections[i].select);
            }

            // ── 콘텐츠 컬럼 (스크롤 본문 + 내부 footer) ──
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE.fill(th.bg_panel().to_egui()))
                .show_inside(ui, |ui| {
                    // footer (디자인 border-top / justify-flex-end / Cancel ghost + Save primary)
                    let footer = egui::TopBottomPanel::bottom("settings_footer")
                        .resizable(false)
                        .show_separator_line(false)
                        .frame(
                            egui::Frame::NONE
                                .fill(th.bg_panel().to_egui())
                                .inner_margin(egui::Margin {
                                    left: SETTINGS_FOOTER_PAD_X,
                                    right: SETTINGS_FOOTER_PAD_X,
                                    top: th.spacing_md.value() as i8,
                                    bottom: th.spacing_md.value() as i8,
                                }),
                        )
                        .show_inside(ui, |ui| {
                            draw_settings_footer(
                                ui,
                                &th,
                                settings,
                                ui_state,
                                file_format,
                                file_handler,
                                user_config_path,
                                &mut result,
                            );
                        });
                    let fr = footer.response.rect;
                    ui.painter().hline(fr.x_range(), fr.top() + 0.5, sep);

                    egui::CentralPanel::default()
                        .frame(egui::Frame::NONE)
                        .show_inside(ui, |ui| {
                            let mut draft = ui_state.draft.take().unwrap();

                            egui::ScrollArea::vertical()
                                .auto_shrink([false, false])
                                .drag_to_scroll(false)
                                .show(ui, |ui| {
                                    tasty_ui_widgets::tab_content_frame(ui, |ui| {
                                        draw_active_content(
                                            ui,
                                            &mut draft,
                                            ui_state,
                                            captured_double_tap,
                                            file_format,
                                            file_handler,
                                        );
                                    });
                                });

                            // 충돌 감지 시 팝업 열기.
                            // intent-exempt: `ui_state.popups` 는 settings 윈도우 내부의
                            // 별도 PopupManager. host Intent 큐(AppState.popups) 와 별개 —
                            // sub-modal 내부 lifecycle 이므로 직접 호출 유지.
                            if ui_state.pending_binding.is_some()
                                && !ui_state.popups.is_open("keybinding_conflict")
                            {
                                ui_state.popups.open_centered_focused("keybinding_conflict");
                            }

                            // 충돌 팝업에서 수락/거부 처리
                            if ui_state.conflict_accepted {
                                ui_state.conflict_accepted = false;
                                if let Some(pending) = ui_state.pending_binding.take() {
                                    draft.keybindings.remove_binding(
                                        &pending.conflicting_field,
                                        pending.conflicting_idx,
                                    );
                                    draft.keybindings.replace_binding_at(
                                        &pending.target_field,
                                        pending.target_idx,
                                        pending.combo,
                                    );
                                }
                                // intent-exempt: settings 윈도우 내부 sub-modal close.
                                ui_state.popups.close("keybinding_conflict");
                            }
                            if ui_state.conflict_cancelled {
                                ui_state.conflict_cancelled = false;
                                ui_state.pending_binding = None;
                                // intent-exempt: settings 윈도우 내부 sub-modal close.
                                ui_state.popups.close("keybinding_conflict");
                            }

                            ui_state.draft = Some(draft);
                        });
                });
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

// ── L2 사이드바 모델 ──────────────────────────────────────────────────────
//
// 디자인의 L2 영속 사이드바는 7 개 L1 탭의 sub-tab 집합을 한 컬럼으로 통합한다.
// 각 row 는 클릭 시 적용할 typed sub-tab 선택값(`L2Select`)을 들고 있어, 셸이
// sub-tab enum 별 분기를 모르고도 선택을 위임할 수 있다.

/// L2 사이드바 한 row 가 클릭됐을 때 적용할 sub-tab 선택값.
enum L2Select {
    General(GeneralSubTab),
    Terminal(TerminalSubTab),
    Appearance(AppearanceSubTab),
    Keybindings(KeybindingsSubTab),
    FileHandler(FileHandlerSubTab),
    Misc(MiscSubTab),
    Plugin(PluginSubTab),
}

/// L2 사이드바 한 row 의 표시 모델.
struct L2Section {
    label: String,
    /// plugin-contributed 섹션이면 true → 라벨 앞에 accent-agent dot.
    is_plugin: bool,
    selected: bool,
    select: L2Select,
}

/// 현재 활성 L1 탭의 L2 섹션 목록을 만든다. Appearance / Plugins 는 plugin 이
/// contribute 한 page 를 동적으로 합성하고, 사라진 page 를 가리키던 sub-tab 은
/// 여기서 리셋한다.
fn build_l2_sections(ui_state: &mut SettingsUiState) -> Vec<L2Section> {
    match ui_state.active_tab {
        SettingsTab::General => {
            let cur = ui_state.general_sub_tab;
            [
                (GeneralSubTab::General, t("settings.tab.general")),
                (
                    GeneralSubTab::Notifications,
                    t("settings.tab.notifications"),
                ),
                (
                    GeneralSubTab::Accessibility,
                    t("settings.misc.subtab.accessibility"),
                ),
            ]
            .into_iter()
            .map(|(tab, label)| L2Section {
                label: label.to_string(),
                is_plugin: false,
                selected: cur == tab,
                select: L2Select::General(tab),
            })
            .collect()
        }
        SettingsTab::Terminal => {
            let cur = ui_state.terminal_sub_tab;
            [
                (TerminalSubTab::General, t("settings.tab.general")),
                (
                    TerminalSubTab::Performance,
                    t("settings.misc.subtab.performance"),
                ),
            ]
            .into_iter()
            .map(|(tab, label)| L2Section {
                label: label.to_string(),
                is_plugin: false,
                selected: cur == tab,
                select: L2Select::Terminal(tab),
            })
            .collect()
        }
        SettingsTab::Appearance => build_appearance_sections(ui_state),
        SettingsTab::Keybindings => {
            let cur = ui_state.keybindings_sub_tab;
            [
                (
                    KeybindingsSubTab::General,
                    t("settings.keybindings.subtab.general"),
                ),
                (
                    KeybindingsSubTab::Workspace,
                    t("settings.keybindings.subtab.workspace"),
                ),
                (
                    KeybindingsSubTab::Pane,
                    t("settings.keybindings.subtab.pane"),
                ),
                (KeybindingsSubTab::Tab, t("settings.keybindings.subtab.tab")),
                (
                    KeybindingsSubTab::Surface,
                    t("settings.keybindings.subtab.surface"),
                ),
                (
                    KeybindingsSubTab::Clipboard,
                    t("settings.keybindings.subtab.clipboard"),
                ),
                (
                    KeybindingsSubTab::Zoom,
                    t("settings.keybindings.subtab.zoom"),
                ),
                (
                    KeybindingsSubTab::Image,
                    t("settings.keybindings.subtab.image"),
                ),
                (
                    KeybindingsSubTab::Explorer,
                    t("settings.keybindings.subtab.explorer"),
                ),
                (
                    KeybindingsSubTab::Scripts,
                    t("settings.keybindings.subtab.scripts"),
                ),
                (
                    KeybindingsSubTab::Preset,
                    t("settings.keybindings.subtab.preset"),
                ),
                (
                    KeybindingsSubTab::Plugins,
                    t("settings.keybindings.subtab.plugins"),
                ),
            ]
            .into_iter()
            .map(|(tab, label)| L2Section {
                label: label.to_string(),
                is_plugin: false,
                selected: cur == tab,
                select: L2Select::Keybindings(tab),
            })
            .collect()
        }
        SettingsTab::FileHandler => {
            let cur = ui_state.file_handler_sub_tab;
            [
                (
                    FileHandlerSubTab::ExtensionMapping,
                    t("settings.file_handler.sub.extension_mapping"),
                ),
                (
                    FileHandlerSubTab::Detectors,
                    t("settings.file_handler.sub.detectors"),
                ),
                (
                    FileHandlerSubTab::Handlers,
                    t("settings.file_handler.sub.handlers"),
                ),
            ]
            .into_iter()
            .map(|(tab, label)| L2Section {
                label: label.to_string(),
                is_plugin: false,
                selected: cur == tab,
                select: L2Select::FileHandler(tab),
            })
            .collect()
        }
        SettingsTab::Misc => {
            // Windows 에서만 Tastyrc 섹션. 비-Windows 는 L2 가 없어 콘텐츠
            // empty-state 가 설명을 대신한다.
            #[cfg(windows)]
            {
                let cur = ui_state.misc_sub_tab;
                vec![L2Section {
                    label: t("settings.misc.subtab.tastyrc").to_string(),
                    is_plugin: false,
                    selected: cur == MiscSubTab::Tastyrc,
                    select: L2Select::Misc(MiscSubTab::Tastyrc),
                }]
            }
            #[cfg(not(windows))]
            {
                let _ = ui_state; // non-windows: tastyrc 서브탭 없어 인자 미사용 — 값 drop(Result 아님).
                Vec::new()
            }
        }
        SettingsTab::Plugins => build_plugin_sections(ui_state),
    }
}

/// Appearance L2: 고정 6 섹션 + Appearance category plugin page.
fn build_appearance_sections(ui_state: &mut SettingsUiState) -> Vec<L2Section> {
    let mut items: Vec<(AppearanceSubTab, String, bool)> = vec![
        (
            AppearanceSubTab::Theme,
            t("settings.appearance.subtab.theme").to_string(),
            false,
        ),
        (
            AppearanceSubTab::Colors,
            t("settings.appearance.subtab.colors").to_string(),
            false,
        ),
        (
            AppearanceSubTab::General,
            t("settings.appearance.subtab.general").to_string(),
            false,
        ),
        (
            AppearanceSubTab::Display,
            t("settings.appearance.subtab.display").to_string(),
            false,
        ),
        (
            AppearanceSubTab::Tasty,
            t("settings.appearance.subtab.tasty").to_string(),
            false,
        ),
        (
            AppearanceSubTab::Terminal,
            t("settings.appearance.subtab.terminal").to_string(),
            false,
        ),
        (
            AppearanceSubTab::Explorer,
            t("settings.appearance.subtab.explorer").to_string(),
            false,
        ),
    ];
    for entry in ui_state
        .settings_pages
        .iter()
        .filter(|e| e.page.category == SettingsCategory::Appearance)
    {
        items.push((
            AppearanceSubTab::Plugin {
                plugin_id: entry.plugin_id.clone(),
                page_id: entry.page.id.clone(),
            },
            t(&entry.page.title_key).to_string(),
            true,
        ));
    }
    // 활성 plugin page 가 사라졌으면 Theme 로 fallback.
    let needs_reset = if let AppearanceSubTab::Plugin {
        plugin_id: ap,
        page_id: pg,
    } = &ui_state.appearance_sub_tab
    {
        !items.iter().any(|(tab, _, _)| {
            matches!(
                tab,
                AppearanceSubTab::Plugin { plugin_id, page_id }
                    if plugin_id == ap && page_id == pg
            )
        })
    } else {
        false
    };
    if needs_reset {
        ui_state.appearance_sub_tab = AppearanceSubTab::Theme;
    }
    let cur = ui_state.appearance_sub_tab.clone();
    items
        .into_iter()
        .map(|(tab, label, is_plugin)| L2Section {
            selected: tab == cur,
            label,
            is_plugin,
            select: L2Select::Appearance(tab),
        })
        .collect()
}

/// Plugins L2: Plugin category page 만으로 구성. 미선택 상태에서 page 가 있으면
/// 첫 page 를 자동 선택한다 (디자인: 진입 시 L2[t][0] 활성).
fn build_plugin_sections(ui_state: &mut SettingsUiState) -> Vec<L2Section> {
    let pages: Vec<(PluginSubTab, String)> = ui_state
        .settings_pages
        .iter()
        .filter(|e| e.page.category == SettingsCategory::Plugin)
        .map(|e| {
            (
                PluginSubTab::Plugin {
                    plugin_id: e.plugin_id.clone(),
                    page_id: e.page.id.clone(),
                },
                t(&e.page.title_key).to_string(),
            )
        })
        .collect();

    let needs_reset = if let Some(PluginSubTab::Plugin {
        plugin_id: ap,
        page_id: pg,
    }) = ui_state.plugin_sub_tab.as_ref()
    {
        !pages.iter().any(|(tab, _)| {
            matches!(
                tab,
                PluginSubTab::Plugin { plugin_id, page_id }
                    if plugin_id == ap && page_id == pg
            )
        })
    } else {
        false
    };
    if needs_reset {
        ui_state.plugin_sub_tab = None;
    }
    if ui_state.plugin_sub_tab.is_none()
        && let Some((first, _)) = pages.first()
    {
        ui_state.plugin_sub_tab = Some(first.clone());
    }

    let cur = ui_state.plugin_sub_tab.clone();
    pages
        .into_iter()
        .map(|(tab, label)| L2Section {
            selected: cur.as_ref() == Some(&tab),
            label,
            is_plugin: true,
            select: L2Select::Plugin(tab),
        })
        .collect()
}

/// L2 row 클릭 → 해당 L1 의 sub-tab 상태에 반영.
fn apply_l2_select(ui_state: &mut SettingsUiState, select: &L2Select) {
    match select {
        L2Select::General(v) => ui_state.general_sub_tab = *v,
        L2Select::Terminal(v) => ui_state.terminal_sub_tab = *v,
        L2Select::Appearance(v) => ui_state.appearance_sub_tab = v.clone(),
        L2Select::Keybindings(v) => {
            ui_state.keybindings_sub_tab = *v;
            // 다른 sub-tab 으로 이동 시 진행 중이던 녹화를 취소.
            ui_state.recording_field = None;
        }
        L2Select::FileHandler(v) => ui_state.file_handler_sub_tab = *v,
        L2Select::Misc(v) => ui_state.misc_sub_tab = *v,
        L2Select::Plugin(v) => ui_state.plugin_sub_tab = Some(v.clone()),
    }
}

// ── L1 헤더 밴드 ──────────────────────────────────────────────────────────

/// 디자인 header band 전사 (settings_window.jsx:465-477): bg-sidebar 위 좌측
/// bold "Settings" 타이틀 + 세로 구분선 → 7 개 L1 탭(active 는 text-primary +
/// 2px accent underline, inactive 는 text-muted) → 우측 `marginLeft:auto` close ✕.
/// close ✕ 클릭 시 `true` 반환(취소와 동일 경로로 모달 닫기 — 사용자 결정: OS
/// 타이틀바와 중복되더라도 디자인대로 카드 내부에 둔다).
fn draw_l1_tab_band(ui: &mut egui::Ui, th: &Theme, ui_state: &mut SettingsUiState) -> bool {
    let tabs = [
        (SettingsTab::General, t("settings.tab.general")),
        (SettingsTab::Terminal, t("settings.tab.terminal")),
        (SettingsTab::Appearance, t("settings.tab.appearance")),
        (SettingsTab::Keybindings, t("settings.tab.keybindings")),
        (SettingsTab::FileHandler, t("settings.tab.file_handler")),
        (SettingsTab::Misc, t("settings.tab.misc")),
        (SettingsTab::Plugins, t("settings.tab.plugin")),
    ];
    let prev = ui_state.active_tab;
    let mut close_clicked = false;
    egui::Frame::NONE
        .inner_margin(egui::Margin {
            left: th.spacing_md.value() as i8,
            right: th.spacing_md.value() as i8,
            top: 0,
            bottom: 0,
        })
        .show(ui, |ui| {
            // 디자인 header band 는 `alignItems:center, height:44`(jsx:465) — 밴드
            // 전체 높이를 가진 영역을 명시 할당하고 `left_to_right(Center)` 로 콘텐츠를
            // 세로 중앙 정렬한다. (`ui.horizontal` 만 쓰면 행이 콘텐츠 높이로 줄어
            // 밴드 상단에 붙어 디자인과 어긋난다.)
            let band_h = ui.available_height();
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), band_h),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    // gap 은 항목마다 명시적으로 add_space — 탭 사이만 2px, 타이틀/구분선
                    // 주변은 디자인 margin(space-sm / size-14) 을 따로 적용한다.
                    ui.spacing_mut().item_spacing.x = 0.0;

                    // 좌측 bold "Settings" 타이틀 (jsx:467, fontSize14 / weight700).
                    ui.label(
                        egui::RichText::new(t("settings.window.title"))
                            .strong()
                            .size(th.font_size_max.value())
                            .color(th.text_primary().to_egui()),
                    );
                    // 타이틀 ↔ 탭 세로 구분선 (jsx:468, width1 h20, margin 좌 space-sm / 우 size-14).
                    ui.add_space(th.spacing_sm.value());
                    let (vrect, _) = ui.allocate_exact_size(
                        egui::vec2(
                            th.border_width.value(),
                            SETTINGS_TITLE_DIVIDER_HEIGHT.value(),
                        ),
                        egui::Sense::hover(),
                    );
                    ui.painter().vline(
                        vrect.center().x,
                        vrect.y_range(),
                        egui::Stroke::new(th.border_width.value(), th.surface1.to_egui()),
                    );
                    ui.add_space(SETTINGS_TITLE_DIVIDER_MARGIN_R.value());

                    // L1 탭들 (gap 2).
                    for (i, (tab, label)) in tabs.into_iter().enumerate() {
                        if i > 0 {
                            ui.add_space(L1_TAB_GAP.value());
                        }
                        if l1_tab_button(ui, th, label, ui_state.active_tab == tab) {
                            ui_state.active_tab = tab;
                        }
                    }

                    // 우측 marginLeft:auto close ✕ (jsx:475, ghost IconButton).
                    let close_side = tasty_ui_widgets::ControlSize::Md.height(th);
                    let pad = (ui.available_width() - close_side).max(0.0);
                    ui.add_space(pad);
                    let resp = tasty_ui_widgets::IconButton::new()
                        .variant(tasty_ui_widgets::IconButtonVariant::Ghost)
                        .show(ui, th, &|ui, rect, c| {
                            crate::adapters::ui::icons::CLOSE
                                .image(rect.width(), c)
                                .paint_at(ui, rect)
                        });
                    if resp.clicked() {
                        close_clicked = true;
                    }
                },
            );
        });
    // L1 전환 시 L2 필터 초기화 (디자인: pickL1 → setFilter("")).
    if ui_state.active_tab != prev {
        ui_state.l2_filter.clear();
    }
    close_clicked
}

/// 헤더 밴드의 한 L1 탭 버튼. 밴드 높이를 가득 채워 active underline 이
/// border-bottom 위치에 정렬되게 한다.
fn l1_tab_button(ui: &mut egui::Ui, th: &Theme, label: &str, active: bool) -> bool {
    let font = egui::FontId::proportional(th.font_size_body.value());
    let pad_x = th.spacing_md.value();
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font, egui::Color32::PLACEHOLDER);
    let galley_size = galley.size();
    let w = galley_size.x + pad_x * 2.0;
    let h = ui.available_height();
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    let color = if active {
        th.text_primary().to_egui()
    } else if resp.hovered() {
        th.text_secondary().to_egui()
    } else {
        th.text_muted().to_egui()
    };
    let pos = egui::pos2(
        rect.center().x - galley_size.x * 0.5,
        rect.center().y - galley_size.y * 0.5,
    );
    ui.painter().galley(pos, galley, color);
    if active {
        let thickness = SETTINGS_TAB_UNDERLINE.value();
        ui.painter().hline(
            rect.x_range(),
            rect.bottom() - thickness * 0.5,
            egui::Stroke::new(thickness, th.accent_primary().to_egui()),
        );
    }
    resp.clicked()
}

// ── L2 사이드바 뷰 ────────────────────────────────────────────────────────

/// 영속 L2 사이드바: 상단 필터 Input(+ border-bottom) + 스크롤 섹션 리스트.
/// 클릭된 섹션 인덱스를 반환한다.
fn draw_l2_sidebar(
    ui: &mut egui::Ui,
    th: &Theme,
    sections: &[L2Section],
    filter: &mut String,
    placeholder: &str,
) -> Option<usize> {
    let mut clicked = None;
    let sep = egui::Stroke::new(th.border_width.value(), th.surface1.to_egui());
    let pad = th.spacing_sm.value() as i8;

    // 필터 입력 — padding space-sm, 하단 border-bottom separator.
    let filter_resp = egui::Frame::NONE
        .inner_margin(egui::Margin::same(pad))
        .show(ui, |ui| {
            // 디자인 settings_window.jsx:484 — leading 돋보기 아이콘(`icon={ic.search}`).
            tasty_ui_widgets::Input::new()
                .placeholder(placeholder)
                .icon(&|ui, rect, c| {
                    crate::adapters::ui::icons::SEARCH
                        .image(rect.width(), c)
                        .paint_at(ui, rect)
                })
                .show(ui, th, filter);
        });
    let frect = filter_resp.response.rect;
    ui.painter()
        .hline(frect.x_range(), frect.bottom() - 0.5, sep);

    // 섹션 리스트 — 스크롤, padding space-sm.
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Frame::NONE
                .inner_margin(egui::Margin::same(pad))
                .show(ui, |ui| {
                    let filter_lc = filter.to_lowercase();
                    let mut any = false;
                    for (i, s) in sections.iter().enumerate() {
                        if !filter_lc.is_empty() && !s.label.to_lowercase().contains(&filter_lc) {
                            continue;
                        }
                        any = true;
                        if sidebar_row(ui, th, &s.label, s.is_plugin, s.selected) {
                            clicked = Some(i);
                        }
                    }
                    // 필터로 0건일 때만 안내. 섹션 자체가 0개(비-Windows Misc)면
                    // 콘텐츠 empty-state 가 대신하므로 사이드바는 비워둔다.
                    if !any && !filter_lc.is_empty() {
                        ui.label(
                            egui::RichText::new(t("settings.filter.no_matches"))
                                .color(th.text_muted().to_egui()),
                        );
                    }
                });
        });
    clicked
}

/// L2 사이드바 한 row. selected = surface-active 배경 + radius-sm, plugin row 는
/// 라벨 앞에 accent-agent dot.
fn sidebar_row(
    ui: &mut egui::Ui,
    th: &Theme,
    label: &str,
    is_plugin: bool,
    selected: bool,
) -> bool {
    let pad_x = th.spacing_sm.value();
    let pad_y = th.spacing_xs.value();
    let font = egui::FontId::proportional(th.font_size_body.value());
    let row_h = th.font_size_body.value() + pad_y * 2.0;
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, row_h), egui::Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    let radius = th.corner_radius_sm.value();
    if selected {
        ui.painter()
            .rect_filled(rect, radius, th.surface_active().to_egui());
    } else if resp.hovered() {
        ui.painter()
            .rect_filled(rect, radius, th.overlay_hover().to_egui_premultiplied());
    }
    let mut x = rect.left() + pad_x;
    if is_plugin {
        let d = th.status_dot_size.value();
        ui.painter().circle_filled(
            egui::pos2(x + d * 0.5, rect.center().y),
            d * 0.5,
            th.accent_agent().to_egui(),
        );
        x += d + th.spacing_sm.value();
    }
    let color = if selected {
        th.text_primary().to_egui()
    } else {
        th.text_muted().to_egui()
    };
    let galley = ui.painter().layout_no_wrap(label.to_owned(), font, color);
    let gy = rect.center().y - galley.size().y * 0.5;
    ui.painter().galley(egui::pos2(x, gy), galley, color);
    resp.clicked()
}

// ── 푸터 ──────────────────────────────────────────────────────────────────

/// 콘텐츠 컬럼 하단 footer: 우측 정렬 Cancel(ghost) + Save(primary).
/// Save 핸들러는 draft commit / scrollback 정리 / 테마 install / file-handler
/// draft 커밋을 수행한다.
#[allow(clippy::too_many_arguments)]
fn draw_settings_footer(
    ui: &mut egui::Ui,
    th: &Theme,
    settings: &mut Settings,
    ui_state: &mut SettingsUiState,
    file_format: &FileFormatRegistry,
    file_handler: &FileHandlerRegistry,
    user_config_path: Option<&std::path::Path>,
    result: &mut Option<bool>,
) {
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = th.spacing_sm.value();
        // RTL: 먼저 추가한 위젯이 가장 우측. 디자인 [Cancel] [Save] (Save 우측).
        if Button::new(t("button.save"))
            .variant(ButtonVariant::Primary)
            .show(ui, th)
            .clicked()
        {
            let prev_restore_surface_content = settings.general.restore_surface_content;
            if let Some(draft) = &ui_state.draft {
                *settings = draft.clone();
            }
            // restore_surface_content 를 끈 경우 기존 scrollback 정리.
            if prev_restore_surface_content && !settings.general.restore_surface_content {
                crate::scrollback_store::clear_all();
            }
            // tasty 빌트인 bashrc 편집은 Windows 전용 (Misc 탭).
            #[cfg(windows)]
            if let Some(bashrc) = &ui_state.bashrc_user_draft {
                crate::settings::general::save_user_bashrc(bashrc);
            }
            // 테마 install 은 여기서 하지 않는다. Save → 모달 close 시
            // `close_active_modal` 이 `UpdateSettings` 인텐트를 큐잉하고,
            // `cascade_settings_updated`(about_to_wait, 렌더 밖)가
            // `install_global_with_zoom` 으로 전역 Theme 를 적용한다.
            // 렌더 클로저는 `draw_settings_panel` 의 `THEME.read()` guard 를
            // 보유 중이므로, 여기서 `set_theme`(=`THEME.write()`)을 호출하면
            // std RwLock self-deadlock 으로 hang 한다. install 은 렌더 밖에서만.
            // FileHandler 탭 편집 draft 를 registry commit + 디스크 저장.
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
            *result = Some(true);
        }
        if Button::new(t("button.cancel"))
            .variant(ButtonVariant::Ghost)
            .show(ui, th)
            .clicked()
        {
            // Cancel: 다음 오픈 시 디스크에서 다시 로드되도록 draft 폐기.
            ui_state.bashrc_user_draft = None;
            ui_state.extension_priority_draft = None;
            ui_state.fh_edit_draft = file_handler_tab::FileHandlerEditDraft::default();
            *result = Some(false);
        }
    });
}

// ── 콘텐츠 디스패치 ───────────────────────────────────────────────────────

/// 활성 L1/L2 에 해당하는 콘텐츠를 그린다. L2 사이드바는 셸이 소유하므로 각 탭
/// draw 는 content-only.
fn draw_active_content(
    ui: &mut egui::Ui,
    draft: &mut Settings,
    ui_state: &mut SettingsUiState,
    captured_double_tap: &mut Option<String>,
    file_format: &FileFormatRegistry,
    file_handler: &FileHandlerRegistry,
) {
    match ui_state.active_tab {
        SettingsTab::General => match ui_state.general_sub_tab {
            GeneralSubTab::General => draw_general_tab(ui, draft),
            GeneralSubTab::Notifications => draw_notifications_tab(ui, draft),
            GeneralSubTab::Accessibility => draw_accessibility_tab(ui, draft),
        },
        SettingsTab::Terminal => match ui_state.terminal_sub_tab {
            TerminalSubTab::General => draw_terminal_tab(ui, draft),
            TerminalSubTab::Performance => draw_performance_tab(ui, draft),
        },
        SettingsTab::Appearance => draw_appearance_tab(
            ui,
            draft,
            &ui_state.appearance_sub_tab,
            &mut ui_state.font_families,
            &mut ui_state.font_filter,
            &mut ui_state.preview_font_loaded,
            &ui_state.settings_pages,
        ),
        SettingsTab::Keybindings => draw_keybindings_tab(
            ui,
            draft,
            &mut ui_state.recording_field,
            ui_state.keybindings_sub_tab,
            &mut ui_state.selected_preset,
            &mut ui_state.pending_binding,
            captured_double_tap,
            &mut ui_state.captured_winit_combo,
            &ui_state.plugin_shortcuts,
            &mut ui_state.plugin_shortcuts_selected,
            &mut ui_state.plugin_shortcuts_draft,
        ),
        SettingsTab::FileHandler => draw_file_handler_tab(
            ui,
            ui_state.file_handler_sub_tab,
            &mut ui_state.extension_priority_draft,
            &mut ui_state.extension_priority_new_input,
            &mut ui_state.fh_edit_draft,
            file_format,
            file_handler,
        ),
        SettingsTab::Misc => draw_misc_content(ui, ui_state),
        SettingsTab::Plugins => draw_plugin_tab(
            ui,
            draft,
            ui_state.plugin_sub_tab.as_ref(),
            &mut ui_state.font_families,
            &mut ui_state.font_filter,
            &mut ui_state.preview_font_loaded,
            &ui_state.settings_pages,
        ),
    }
}

/// Misc 콘텐츠. Windows = tasty 빌트인 bashrc 편집, 비-Windows = empty-state.
fn draw_misc_content(ui: &mut egui::Ui, ui_state: &mut SettingsUiState) {
    match ui_state.misc_sub_tab {
        #[cfg(windows)]
        MiscSubTab::Tastyrc => draw_tastyrc_subtab(ui, &mut ui_state.bashrc_user_draft),
        #[cfg(not(windows))]
        MiscSubTab::Tastyrc => {
            let th = crate::theme::theme();
            ui.vertical_centered(|ui| {
                ui.add_space(24.0);
                ui.label(
                    egui::RichText::new(t("settings.misc.empty")).color(th.text_muted().to_egui()),
                );
            });
        }
    }
}
