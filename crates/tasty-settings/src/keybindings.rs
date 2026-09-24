use serde::{Deserialize, Serialize};

/// 탭 전환의 필드 타입·기본값·slot_count가 공유하는 슬롯 수.
pub const TAB_SWITCH_SLOT_COUNT: usize = 10;
/// 워크스페이스 quick-switch 슬롯 수(0번 슬롯 없음 — 기존 정책).
pub const WORKSPACE_SWITCH_SLOT_COUNT: usize = 9;
/// 카테고리 quick-switch 슬롯 수(1~9 후 0 = 10번째).
pub const CATEGORY_SWITCH_SLOT_COUNT: usize = 10;

/// 스크립트 ID에 연결할 단축키 하나. 고정 액션 목록과 별도로 저장한다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScriptBinding {
    pub script_id: String,
    pub combo: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct KeybindingSettings {
    pub new_workspace: Vec<String>,
    pub new_tab: Vec<String>,
    pub split_pane_vertical: Vec<String>,
    pub split_pane_horizontal: Vec<String>,
    pub split_surface_vertical: Vec<String>,
    pub split_surface_horizontal: Vec<String>,
    pub toggle_settings: Vec<String>,
    pub toggle_notifications: Vec<String>,
    /// DAG 목록 popup 토글. 활성 workspace 스코프로 열린다.
    pub toggle_dag_list: Vec<String>,
    // 도구 메뉴의 다섯 동작은 기본 단축키를 비워 두고 사용자가 지정한다.
    /// 포트 스캐너 popup 열기 (도구 메뉴).
    pub open_port_scanner: Vec<String>,
    /// 원격 도구 popup 열기 (도구 메뉴).
    pub open_remote_tool: Vec<String>,
    /// Preset 윈도우 열기 (도구 메뉴). **레이아웃 프리셋 적용(`apply_*_preset`)과 다르다** —
    /// 이쪽은 프리셋을 만들고 고치는 별도 winit 윈도우다.
    pub open_preset_window: Vec<String>,
    /// 튜토리얼 주제 popup 열기 (도구 메뉴).
    pub open_tutorial: Vec<String>,
    /// 파일 피커 popup 열기 (도구 메뉴).
    pub open_file_picker: Vec<String>,
    pub close_pane: Vec<String>,
    pub close_surface: Vec<String>,
    pub close_workspace: Vec<String>,
    pub focus_pane_next: Vec<String>,
    pub focus_pane_prev: Vec<String>,
    pub focus_surface_next: Vec<String>,
    pub focus_surface_prev: Vec<String>,
    /// Modifier **combo** for tab switch (number keys). 단일 토큰(`"ctrl"`) 또는
    /// 조합(`"ctrl+shift"`)을 허용한다 — 매칭은 `parse_binding` 기반 4축 조합으로 한다.
    pub tab_switch_modifier: String,
    /// Modifier **combo** for workspace switch (number keys). 단일 토큰 또는 조합.
    pub workspace_switch_modifier: String,
    /// 카테고리 전환 modifier. 필드가 없으면 default_category_switch_modifier를 사용한다.
    #[serde(default = "default_category_switch_modifier")]
    pub category_switch_modifier: String,
    /// 전체화면 무대가 활성일 때만 검사하는 종료 키. 무대가 없으면 다른 Escape 동작을 가로채지 않는다.
    /// 필드가 없으면 default_fullscreen_stage_exit를 사용한다.
    #[serde(default = "default_fullscreen_stage_exit")]
    pub fullscreen_stage_exit: Vec<String>,
    /// Toggle sidebar visibility (completely hidden/shown).
    pub toggle_sidebar: Vec<String>,
    /// Toggle sidebar collapse (full/compact mode).
    pub toggle_sidebar_collapse: Vec<String>,
    /// Collapse/expand all workspace categories at once (any expanded → collapse all,
    /// all collapsed → expand all). No-op when workspace categories are disabled.
    pub toggle_categories_collapsed: Vec<String>,
    /// Restore the most recently closed surface/tab/workspace.
    pub restore_closed: Vec<String>,
    /// Quit: follows close_behavior setting (ask/minimize/quit).
    pub quit: Vec<String>,
    /// Immediate quit: force exit, close everything.
    pub quit_immediate: Vec<String>,
    /// Minimize to background (park state).
    pub quit_minimize: Vec<String>,
    /// Open Markdown viewer (shows path dialog).
    pub open_markdown: Vec<String>,
    /// Open file Explorer tab.
    pub open_explorer: Vec<String>,
    /// Open Surface type convert popup.
    pub convert_surface: Vec<String>,
    /// Direct convert to Markdown (shows path dialog).
    pub convert_to_markdown: Vec<String>,
    /// Direct convert to Explorer.
    pub convert_to_explorer: Vec<String>,
    /// Open a new window.
    pub new_window: Vec<String>,
    /// Close nearest: tab → pane → workspace.
    pub close_active: Vec<String>,
    /// Focus next tab in the current pane.
    pub next_tab: Vec<String>,
    /// Focus previous tab in the current pane.
    pub prev_tab: Vec<String>,
    /// 화면 캡처 파일의 경로를 클립보드에 복사한다. 원격 mirror에서는 파일을 원격으로 보내
    /// 원격 클립보드에 기록한다. 필드가 없으면 default_screenshot_to_clipboard를 사용한다.
    #[serde(default = "default_screenshot_to_clipboard")]
    pub screenshot_to_clipboard: Vec<String>,
    /// Open terminal text search bar.
    pub find: Vec<String>,
    /// Copy selection (or inject egui Copy event) from focused surface.
    pub copy: Vec<String>,
    /// Copy selected file paths as text (Explorer only).
    pub copy_path: Vec<String>,
    /// Cut selected files (Explorer only).
    pub cut: Vec<String>,
    /// Select all files (Explorer only).
    pub select_all: Vec<String>,
    /// Reload the focused Explorer directory listing.
    pub explorer_refresh: Vec<String>,
    /// Navigate the focused Explorer to the parent directory.
    pub explorer_go_up: Vec<String>,
    /// Paste clipboard content into focused terminal / paste files in Explorer.
    pub paste: Vec<String>,
    /// Increase font size.
    pub zoom_in: Vec<String>,
    /// Decrease font size.
    pub zoom_out: Vec<String>,
    /// Reset font size.
    pub zoom_reset: Vec<String>,
    /// Open the rename dialog for the focused tab.
    pub rename_tab: Vec<String>,
    /// Open the name rename dialog for the active workspace.
    pub rename_workspace: Vec<String>,
    /// Open the subtitle rename dialog for the active workspace.
    pub rename_workspace_subtitle: Vec<String>,
    /// Toggle the command palette popup.
    pub toggle_command_palette: Vec<String>,
    /// Open the Apply workspace preset picker.
    pub apply_workspace_preset: Vec<String>,
    /// Open the Apply tab preset picker.
    pub apply_tab_preset: Vec<String>,
    /// Open the Apply pane preset picker.
    pub apply_pane_preset: Vec<String>,
    /// Enter vi-style keyboard copy mode in the focused terminal.
    pub enter_copy_mode: Vec<String>,
    /// Minimize the current window (CSD caption / native traffic light parity).
    pub minimize_window: Vec<String>,
    /// Toggle maximize/restore the current window (macOS: zoom).
    pub maximize_window: Vec<String>,
    /// Close the current window.
    pub close_window: Vec<String>,
    /// 사용자 스크립트↔단축키 동적 바인딩 (docs/features/lua-hooks/index.md#실행-격리--안전-장치). 고정 필드와 별개 표현.
    /// `#[serde(default)]` 로 기존 config 마이그레이션 안전(누락 시 빈 목록).
    #[serde(default)]
    pub script_bindings: Vec<ScriptBinding>,
    /// 탭 전환 슬롯의 기본 raw 키는 1~9, 0이다. modifier는 실행 시 조합한다.
    /// 필드가 없으면 default_tab_slot_keys를 사용한다.
    #[serde(default = "default_tab_slot_keys")]
    pub tab_switch_slot_keys: [String; TAB_SWITCH_SLOT_COUNT],
    /// 워크스페이스 quick-switch 슬롯 1~9번의 raw 키(0번 슬롯 없음 — 기존 정책 유지).
    /// dispatch 시점에 `workspace_switch_modifier` 와 조합된다. 기본값 `["1".."9"]`.
    #[serde(default = "default_workspace_slot_keys")]
    pub workspace_switch_slot_keys: [String; WORKSPACE_SWITCH_SLOT_COUNT],
    /// 카테고리 quick-switch 슬롯 1~10번의 raw 키(1~9 후 0 = 10번째). dispatch 시점에
    /// `category_switch_modifier`(기본 `ctrl+shift`) 와 조합된다. folders 기능 on 일 때만
    /// 유효. 기본값 `["1".."9","0"]`.
    #[serde(default = "default_category_slot_keys")]
    pub category_switch_slot_keys: [String; CATEGORY_SWITCH_SLOT_COUNT],
    /// 탭 quick-switch "다음 탭" raw 키. 기본값 `"l"`(vim). `next_tab` 과 별개 필드.
    #[serde(default = "default_tab_next_key")]
    pub tab_switch_next_key: String,
    /// 탭 quick-switch "이전 탭" raw 키. 기본값 `"h"`(vim). `prev_tab` 과 별개 필드.
    #[serde(default = "default_tab_prev_key")]
    pub tab_switch_prev_key: String,
    /// 워크스페이스 quick-switch "다음" raw 키. 기본값 `"j"`(vim).
    #[serde(default = "default_workspace_next_key")]
    pub workspace_switch_next_key: String,
    /// 워크스페이스 quick-switch "이전" raw 키. 기본값 `"k"`(vim).
    #[serde(default = "default_workspace_prev_key")]
    pub workspace_switch_prev_key: String,
    /// 카테고리 다음 전환 키. 기본 j이며 modifier는 category_switch_modifier를 사용한다.
    #[serde(default = "default_category_next_key")]
    pub category_switch_next_key: String,
    /// 카테고리 quick-switch "이전 카테고리" raw 키. 기본값 `"k"`(vim).
    #[serde(default = "default_category_prev_key")]
    pub category_switch_prev_key: String,
}

/// 누락된 탭 슬롯 필드에 사용할 1~9, 0 기본값.
fn default_tab_slot_keys() -> [String; TAB_SWITCH_SLOT_COUNT] {
    ["1", "2", "3", "4", "5", "6", "7", "8", "9", "0"].map(String::from)
}

/// 워크스페이스 quick-switch 슬롯 raw 키 기본값 `["1".."9"]`(0번 슬롯 없음).
fn default_workspace_slot_keys() -> [String; WORKSPACE_SWITCH_SLOT_COUNT] {
    ["1", "2", "3", "4", "5", "6", "7", "8", "9"].map(String::from)
}

/// 카테고리 quick-switch 슬롯 raw 키 기본값 `["1".."9","0"]`(1~9 후 0 = 10번째).
fn default_category_slot_keys() -> [String; CATEGORY_SWITCH_SLOT_COUNT] {
    ["1", "2", "3", "4", "5", "6", "7", "8", "9", "0"].map(String::from)
}

/// 탭 quick-switch "다음 탭" 기본 키 `"l"`(vim).
fn default_tab_next_key() -> String {
    "l".to_string()
}

/// 탭 quick-switch "이전 탭" 기본 키 `"h"`(vim).
fn default_tab_prev_key() -> String {
    "h".to_string()
}

/// 워크스페이스 quick-switch "다음" 기본 키 `"j"`(vim).
fn default_workspace_next_key() -> String {
    "j".to_string()
}

/// 워크스페이스 quick-switch "이전" 기본 키 `"k"`(vim).
fn default_workspace_prev_key() -> String {
    "k".to_string()
}

/// 카테고리 quick-switch "다음 카테고리" 기본 키 `"j"`(vim).
fn default_category_next_key() -> String {
    "j".to_string()
}

/// 카테고리 quick-switch "이전 카테고리" 기본 키 `"k"`(vim).
fn default_category_prev_key() -> String {
    "k".to_string()
}

/// 누락된 카테고리 modifier의 기본값. 저장 토큰 ctrl+shift를 사용한다.
fn default_category_switch_modifier() -> String {
    "ctrl+shift".to_string()
}

/// 누락된 화면 캡처 바인딩의 기본값. 네 프리셋이 같은 조합을 사용한다.
fn default_screenshot_to_clipboard() -> Vec<String> {
    vec!["ctrl+alt+s".to_string()]
}

/// 누락된 전체화면 무대 종료 바인딩의 기본값.
fn default_fullscreen_stage_exit() -> Vec<String> {
    vec!["escape".to_string()]
}

impl KeybindingSettings {
    /// 슬롯별 완전한 단축키를 쓰는 개별 지정 모드의 저장값.
    /// ctrl/alt/option/shift 조합으로 파싱되지 않아 규칙 기반 모드와 구분된다.
    pub const INDIVIDUAL_SWITCH_MODIFIER: &'static str = "individual";
}

impl Default for KeybindingSettings {
    fn default() -> Self {
        Self::preset_tasty()
    }
}

pub mod crud;
/// 바인딩 문자열 파서와 modifier 조합 — 이 크레이트가 저장하는 값의 해석 규칙.
pub mod parse;
mod presets;

#[cfg(test)]
#[path = "keybindings/tests.rs"]
mod tests;
