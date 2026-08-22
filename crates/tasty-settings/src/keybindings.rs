use serde::{Deserialize, Serialize};

/// 사용자 스크립트↔단축키 동적 바인딩 (ADR-0031).
///
/// 고정 액션 필드(`Vec<String>`)와 달리 스크립트는 N 개 동적이라 별도 표현이 필요하다.
/// 스크립트당 combo 하나(디자인 05: 행마다 Kbd 1개). `script_id` 는 `ScriptRegistry`(03) 참조.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScriptBinding {
    pub script_id: String,
    pub combo: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    /// Modifier **combo** for category switch (number keys). 세 축 중 카테고리 축의
    /// 독립 modifier — 과거 `workspace_switch_modifier`+Shift 파생을 대체한 1급 필드.
    /// 기본값 `"ctrl+shift"`(macOS 스크린샷 `⌘⇧3/4/5` 예약과 겹치지 않음).
    ///
    /// ⚠️ 신규 필드라 구 config 에는 없다 — struct 레벨 `#[serde(default)]` 만으로는
    /// `String::default()`(빈 문자열)로 채워져 매칭이 조용히 죽는다. 전용 default fn 필수.
    #[serde(default = "default_category_switch_modifier")]
    pub category_switch_modifier: String,
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
    /// (03) 화면을 인터랙티브하게 캡처해 경로를 클립보드에 복사한다. 포커스된
    /// surface 가 원격 attach(mirror) workspace 소속이면 캡처 파일을 원격으로
    /// 전송해 원격 클립보드에 경로를 기록한다(로컬이면 로컬 클립보드).
    ///
    /// ⚠️ 신규 필드라 구 config 에는 없다 — struct 레벨 `#[serde(default)]` 만으로는
    /// `Vec::default()`(빈 벡터)로 채워져 기본 바인딩이 조용히 사라진다. 전용
    /// default fn 필수(`category_switch_modifier` 선례).
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
    /// Undo in image editor.
    pub image_undo: Vec<String>,
    /// Redo in image editor.
    pub image_redo: Vec<String>,
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
    /// 사용자 스크립트↔단축키 동적 바인딩 (ADR-0031). 고정 필드와 별개 표현.
    /// `#[serde(default)]` 로 기존 config 마이그레이션 안전(누락 시 빈 목록).
    #[serde(default)]
    pub script_bindings: Vec<ScriptBinding>,
    /// 탭 quick-switch 슬롯 1~10번의 raw 키(modifier 없음). dispatch 시점에
    /// `tab_switch_modifier` 와 조합된다(quickswitch-03). 기본값 `["1".."9","0"]`.
    ///
    /// ⚠️ 필드별 default fn 필수 — struct 레벨 `#[serde(default)]` 만으로는 누락 시
    /// `[String;10]::default()`(빈 문자열 10개)로 채워져 기존 config 가 조용히 깨진다.
    #[serde(default = "default_tab_slot_keys")]
    pub tab_switch_slot_keys: [String; 10],
    /// 워크스페이스 quick-switch 슬롯 1~9번의 raw 키(0번 슬롯 없음 — 기존 정책 유지).
    /// dispatch 시점에 `workspace_switch_modifier` 와 조합된다. 기본값 `["1".."9"]`.
    #[serde(default = "default_workspace_slot_keys")]
    pub workspace_switch_slot_keys: [String; 9],
    /// 카테고리 quick-switch 슬롯 1~10번의 raw 키(1~9 후 0 = 10번째). dispatch 시점에
    /// `category_switch_modifier`(기본 `ctrl+shift`) 와 조합된다. folders 기능 on 일 때만
    /// 유효. 기본값 `["1".."9","0"]`.
    #[serde(default = "default_category_slot_keys")]
    pub category_switch_slot_keys: [String; 10],
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
    /// 카테고리 quick-switch "다음 카테고리" raw 키. 기본값 `"j"`(vim). 4프리셋 전수
    /// 대조로 무충돌 확인된 값(워크스페이스 축과 문자는 같지만 modifier 가 달라
    /// 합성 콤보는 겹치지 않는다 — `ctrl+shift+j` vs `alt+j`).
    #[serde(default = "default_category_next_key")]
    pub category_switch_next_key: String,
    /// 카테고리 quick-switch "이전 카테고리" raw 키. 기본값 `"k"`(vim).
    #[serde(default = "default_category_prev_key")]
    pub category_switch_prev_key: String,
}

/// 탭 quick-switch 슬롯 raw 키 기본값 `["1".."9","0"]`(현행 `TAB_DIGITS` 와 동일).
///
/// 기존 config 마이그레이션 안전용. struct 레벨 `#[serde(default)]` 는 누락 필드를
/// 그 타입의 `Default::default()`(= 빈 문자열 배열)로 채우므로, 필드별 전용 default fn 이
/// 없으면 quick-switch 가 조용히 무효화된다. (`appearance.rs` `default_ligatures` 선례.)
fn default_tab_slot_keys() -> [String; 10] {
    ["1", "2", "3", "4", "5", "6", "7", "8", "9", "0"].map(String::from)
}

/// 워크스페이스 quick-switch 슬롯 raw 키 기본값 `["1".."9"]`(0번 슬롯 없음).
fn default_workspace_slot_keys() -> [String; 9] {
    ["1", "2", "3", "4", "5", "6", "7", "8", "9"].map(String::from)
}

/// 카테고리 quick-switch 슬롯 raw 키 기본값 `["1".."9","0"]`(1~9 후 0 = 10번째).
fn default_category_slot_keys() -> [String; 10] {
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

/// 카테고리 quick-switch modifier 조합 기본값 `"ctrl+shift"`.
///
/// 4 프리셋 공통. macOS 시스템 스크린샷 예약(`⌘⇧3/4/5/6`, tasty 가 가로챌 수 없음)과
/// 겹치지 않는 안전한 조합이다. 구 config(카테고리 필드 없음) 로드 시 이 값으로 채워진다.
fn default_category_switch_modifier() -> String {
    "ctrl+shift".to_string()
}

/// (03) 스크린샷→클립보드 기본 바인딩 `"ctrl+alt+s"`. 4 프리셋 공통 — macOS
/// 시스템 스크린샷 예약(`⌘⇧3/4/5/6`, 이 스킴의 `alt+shift+3/4/5/6`)과 겹치지
/// 않는다(어느 프리셋도 그 조합을 안 씀). 구 config(필드 없음) 로드 시 이 값으로 채워진다.
fn default_screenshot_to_clipboard() -> Vec<String> {
    vec!["ctrl+alt+s".to_string()]
}

impl KeybindingSettings {
    /// "개별 지정" sentinel — `tab_switch_modifier`/`workspace_switch_modifier`/
    /// `category_switch_modifier` 에 저장되면 그 축이 규칙 기반(modifier + raw 키 1개
    /// 조합) 대신 **슬롯마다 독립된 완전 콤보**(모디파이어 포함 자유 조합)를 쓴다는
    /// 뜻이다. 4축 조합 파서([`crate`] 밖 `Combo::parse_modifiers`, `src/adapters/ui/
    /// input/shortcuts/modifier_hint.rs`)는 `ctrl`/`shift`/`alt`/`option` 토큰만
    /// 인식하고 그 외 토큰이 섞이면 무조건 `None` 을 반환하므로, 이 문자열은 파서
    /// 수정 없이도 "규칙 기반 조합이 아니다" 를 안전하게 표현한다(어떤 4축 조합의
    /// `Combo::name()` 결과와도 겹치지 않음).
    pub const INDIVIDUAL_SWITCH_MODIFIER: &'static str = "individual";
}

impl Default for KeybindingSettings {
    fn default() -> Self {
        Self::preset_tasty()
    }
}

mod crud;
mod presets;

#[cfg(test)]
#[path = "keybindings/tests.rs"]
mod tests;
