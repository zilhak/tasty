use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct KeybindingSettings {
    pub new_workspace: String,
    pub new_tab: String,
    pub split_pane_vertical: String,
    pub split_pane_horizontal: String,
    pub split_surface_vertical: String,
    pub split_surface_horizontal: String,
    pub toggle_settings: String,
    pub toggle_notifications: String,
    pub close_pane: String,
    pub close_surface: String,
    pub close_workspace: String,
    pub focus_pane_next: String,
    pub focus_pane_prev: String,
    pub focus_surface_next: String,
    pub focus_surface_prev: String,
    /// Modifier for tab switch (number keys): "ctrl" or "alt"
    pub tab_switch_modifier: String,
    /// Modifier for workspace switch (number keys): "ctrl" or "alt"
    pub workspace_switch_modifier: String,
    /// Toggle sidebar visibility (completely hidden/shown).
    pub toggle_sidebar: String,
    /// Toggle sidebar collapse (full/compact mode).
    pub toggle_sidebar_collapse: String,
    /// Restore the most recently closed surface/tab/workspace.
    pub restore_closed: String,
    /// Quit: follows close_behavior setting (ask/minimize/quit).
    pub quit: String,
    /// Immediate quit: force exit, close everything.
    pub quit_immediate: String,
    /// Minimize to background (park state).
    pub quit_minimize: String,
    /// Open Markdown viewer (shows path dialog).
    pub open_markdown: String,
    /// Open file Explorer tab.
    pub open_explorer: String,
    /// Open Surface type convert popup.
    pub convert_surface: String,
    /// Direct convert to Markdown (shows path dialog).
    pub convert_to_markdown: String,
    /// Direct convert to Explorer.
    pub convert_to_explorer: String,
    /// Open a new window.
    pub new_window: String,
    /// Close nearest: tab → pane → workspace.
    pub close_active: String,
    /// Focus next tab in the current pane.
    pub next_tab: String,
    /// Focus previous tab in the current pane.
    pub prev_tab: String,
}

impl KeybindingSettings {
    /// 일반 단축키 필드 전체 목록 (modifier 필드 제외).
    /// 중복 검사 및 field_id ↔ 라벨 매핑에 사용.
    pub const GENERAL_BINDING_FIELDS: &'static [(&'static str, &'static str)] = &[
        ("new_workspace",           "settings.keybindings.new_workspace_label"),
        ("new_tab",                 "settings.keybindings.new_tab_label"),
        ("split_pane_vertical",     "settings.keybindings.split_pane_vertical_label"),
        ("split_pane_horizontal",   "settings.keybindings.split_pane_horizontal_label"),
        ("split_surface_vertical",  "settings.keybindings.split_surface_vertical_label"),
        ("split_surface_horizontal","settings.keybindings.split_surface_horizontal_label"),
        ("toggle_settings",         "settings.keybindings.toggle_settings_label"),
        ("toggle_notifications",    "settings.keybindings.toggle_notifications_label"),
        ("close_pane",              "settings.keybindings.close_pane_label"),
        ("close_surface",           "settings.keybindings.close_surface_label"),
        ("close_workspace",         "settings.keybindings.close_workspace_label"),
        ("focus_pane_next",         "settings.keybindings.focus_pane_next_label"),
        ("focus_pane_prev",         "settings.keybindings.focus_pane_prev_label"),
        ("focus_surface_next",      "settings.keybindings.focus_surface_next_label"),
        ("focus_surface_prev",      "settings.keybindings.focus_surface_prev_label"),
        ("restore_closed",          "settings.keybindings.restore_closed_label"),
        ("quit",                    "settings.keybindings.quit_label"),
        ("quit_immediate",          "settings.keybindings.quit_immediate_label"),
        ("quit_minimize",           "settings.keybindings.quit_minimize_label"),
        ("open_markdown",           "settings.keybindings.open_markdown_label"),
        ("open_explorer",           "settings.keybindings.open_explorer_label"),
        ("convert_surface",         "settings.keybindings.convert_surface_label"),
        ("convert_to_markdown",     "settings.keybindings.convert_to_markdown_label"),
        ("convert_to_explorer",     "settings.keybindings.convert_to_explorer_label"),
        ("new_window",              "settings.keybindings.new_window_label"),
        ("close_active",            "settings.keybindings.close_active_label"),
        ("next_tab",                "settings.keybindings.next_tab_label"),
        ("prev_tab",                "settings.keybindings.prev_tab_label"),
    ];

    pub fn get_field(&self, field_id: &str) -> Option<&str> {
        Some(match field_id {
            "new_workspace"            => self.new_workspace.as_str(),
            "new_tab"                  => self.new_tab.as_str(),
            "split_pane_vertical"      => self.split_pane_vertical.as_str(),
            "split_pane_horizontal"    => self.split_pane_horizontal.as_str(),
            "split_surface_vertical"   => self.split_surface_vertical.as_str(),
            "split_surface_horizontal" => self.split_surface_horizontal.as_str(),
            "toggle_settings"          => self.toggle_settings.as_str(),
            "toggle_notifications"     => self.toggle_notifications.as_str(),
            "close_pane"               => self.close_pane.as_str(),
            "close_surface"            => self.close_surface.as_str(),
            "close_workspace"          => self.close_workspace.as_str(),
            "focus_pane_next"          => self.focus_pane_next.as_str(),
            "focus_pane_prev"          => self.focus_pane_prev.as_str(),
            "focus_surface_next"       => self.focus_surface_next.as_str(),
            "focus_surface_prev"       => self.focus_surface_prev.as_str(),
            "toggle_sidebar"           => self.toggle_sidebar.as_str(),
            "toggle_sidebar_collapse"  => self.toggle_sidebar_collapse.as_str(),
            "restore_closed"           => self.restore_closed.as_str(),
            "quit"                     => self.quit.as_str(),
            "quit_immediate"           => self.quit_immediate.as_str(),
            "quit_minimize"            => self.quit_minimize.as_str(),
            "open_markdown"            => self.open_markdown.as_str(),
            "open_explorer"            => self.open_explorer.as_str(),
            "convert_surface"          => self.convert_surface.as_str(),
            "convert_to_markdown"      => self.convert_to_markdown.as_str(),
            "convert_to_explorer"      => self.convert_to_explorer.as_str(),
            "new_window"               => self.new_window.as_str(),
            "close_active"             => self.close_active.as_str(),
            "next_tab"                 => self.next_tab.as_str(),
            "prev_tab"                 => self.prev_tab.as_str(),
            _ => return None,
        })
    }

    pub fn set_field(&mut self, field_id: &str, value: &str) -> bool {
        let target: &mut String = match field_id {
            "new_workspace"            => &mut self.new_workspace,
            "new_tab"                  => &mut self.new_tab,
            "split_pane_vertical"      => &mut self.split_pane_vertical,
            "split_pane_horizontal"    => &mut self.split_pane_horizontal,
            "split_surface_vertical"   => &mut self.split_surface_vertical,
            "split_surface_horizontal" => &mut self.split_surface_horizontal,
            "toggle_settings"          => &mut self.toggle_settings,
            "toggle_notifications"     => &mut self.toggle_notifications,
            "close_pane"               => &mut self.close_pane,
            "close_surface"            => &mut self.close_surface,
            "close_workspace"          => &mut self.close_workspace,
            "focus_pane_next"          => &mut self.focus_pane_next,
            "focus_pane_prev"          => &mut self.focus_pane_prev,
            "focus_surface_next"       => &mut self.focus_surface_next,
            "focus_surface_prev"       => &mut self.focus_surface_prev,
            "toggle_sidebar"           => &mut self.toggle_sidebar,
            "toggle_sidebar_collapse"  => &mut self.toggle_sidebar_collapse,
            "restore_closed"           => &mut self.restore_closed,
            "quit"                     => &mut self.quit,
            "quit_immediate"           => &mut self.quit_immediate,
            "quit_minimize"            => &mut self.quit_minimize,
            "open_markdown"            => &mut self.open_markdown,
            "open_explorer"            => &mut self.open_explorer,
            "convert_surface"          => &mut self.convert_surface,
            "convert_to_markdown"      => &mut self.convert_to_markdown,
            "convert_to_explorer"      => &mut self.convert_to_explorer,
            "new_window"               => &mut self.new_window,
            "close_active"             => &mut self.close_active,
            "next_tab"                 => &mut self.next_tab,
            "prev_tab"                 => &mut self.prev_tab,
            _ => return false,
        };
        *target = value.to_string();
        true
    }

    pub fn clear_field(&mut self, field_id: &str) -> bool {
        self.set_field(field_id, "")
    }

    /// `combo`와 같은 값을 가진 **다른 필드**의 field_id를 반환.
    /// 빈 조합은 항상 None. 자기 자신(field_id 일치)은 제외.
    pub fn find_conflict(&self, field_id: &str, combo: &str) -> Option<&'static str> {
        if combo.is_empty() {
            return None;
        }
        for (id, _label) in Self::GENERAL_BINDING_FIELDS {
            if *id == field_id {
                continue;
            }
            if self.get_field(id).is_some_and(|v| v == combo) {
                return Some(id);
            }
        }
        None
    }

    /// field_id → 라벨 번역 키.
    pub fn label_key_for(field_id: &str) -> Option<&'static str> {
        Self::GENERAL_BINDING_FIELDS
            .iter()
            .find(|(id, _)| *id == field_id)
            .map(|(_, key)| *key)
    }

    /// Format a binding string for display (e.g. "ctrl+shift+n" → "Ctrl+Shift+N").
    pub fn format_display(binding: &str) -> String {
        if binding.is_empty() {
            return String::new();
        }
        binding
            .split('+')
            .map(|part| {
                let mut chars = part.chars();
                match chars.next() {
                    Some(first) => {
                        let upper = first.to_uppercase().to_string();
                        format!("{}{}", upper, chars.as_str())
                    }
                    None => String::new(),
                }
            })
            .collect::<Vec<_>>()
            .join("+")
    }
}

impl Default for KeybindingSettings {
    fn default() -> Self {
        Self::preset_tasty()
    }
}

impl KeybindingSettings {
    /// Tasty preset (default). On macOS, Alt maps to Cmd.
    pub fn preset_tasty() -> Self {
        Self {
            new_workspace: "alt+n".to_string(),
            new_tab: "alt+t".to_string(),
            split_pane_vertical: "alt+e".to_string(),
            split_pane_horizontal: "alt+shift+e".to_string(),
            split_surface_vertical: "alt+d".to_string(),
            split_surface_horizontal: "alt+shift+d".to_string(),
            toggle_settings: "ctrl+,".to_string(),
            toggle_notifications: "ctrl+shift+i".to_string(),
            close_pane: "ctrl+shift+w".to_string(),
            close_surface: String::new(),
            close_workspace: "alt+shift+w".to_string(),
            focus_pane_next: "ctrl+]".to_string(),
            focus_pane_prev: "ctrl+[".to_string(),
            focus_surface_next: "alt+]".to_string(),
            focus_surface_prev: "alt+[".to_string(),
            tab_switch_modifier: "ctrl".to_string(),
            workspace_switch_modifier: "alt".to_string(),
            toggle_sidebar: "ctrl+shift+b".to_string(),
            toggle_sidebar_collapse: "ctrl+b".to_string(),
            restore_closed: "ctrl+shift+t".to_string(),
            quit: String::new(),
            quit_immediate: String::new(),
            quit_minimize: String::new(),
            open_markdown: String::new(),
            open_explorer: String::new(),
            convert_surface: "alt+'".to_string(),
            convert_to_markdown: String::new(),
            convert_to_explorer: String::new(),
            new_window: "alt+shift+n".to_string(),
            close_active: "ctrl+w".to_string(),
            next_tab: "ctrl+tab".to_string(),
            prev_tab: "ctrl+shift+tab".to_string(),
        }
    }

    /// List available preset names.
    pub fn preset_names() -> &'static [&'static str] {
        &["Tasty"]
    }

    /// 이름으로 프리셋의 원본 인스턴스를 얻는다. 미리보기/적용 공통 소스.
    pub fn preset_by_name(name: &str) -> Option<Self> {
        match name {
            "Tasty" => Some(Self::preset_tasty()),
            _ => None,
        }
    }

    /// Apply a preset by name. Returns true if found.
    pub fn apply_preset(&mut self, name: &str) -> bool {
        match Self::preset_by_name(name) {
            Some(preset) => {
                *self = preset;
                true
            }
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preset_by_name_matches_preset_tasty() {
        let by_name = KeybindingSettings::preset_by_name("Tasty").unwrap();
        let direct = KeybindingSettings::preset_tasty();
        for (id, _) in KeybindingSettings::GENERAL_BINDING_FIELDS {
            assert_eq!(
                by_name.get_field(id),
                direct.get_field(id),
                "field {id} mismatch"
            );
        }
        assert!(KeybindingSettings::preset_by_name("Unknown").is_none());
    }

    #[test]
    fn find_conflict_detects_duplicate_across_fields() {
        let kb = KeybindingSettings::preset_tasty();
        assert_eq!(
            kb.find_conflict("toggle_settings", "ctrl+shift+w"),
            Some("close_pane")
        );
    }

    #[test]
    fn find_conflict_ignores_self() {
        let kb = KeybindingSettings::preset_tasty();
        assert_eq!(kb.find_conflict("close_pane", "ctrl+shift+w"), None);
    }

    #[test]
    fn find_conflict_empty_combo_is_none() {
        let kb = KeybindingSettings::preset_tasty();
        assert_eq!(kb.find_conflict("close_pane", ""), None);
    }

    #[test]
    fn find_conflict_empty_fields_dont_conflict() {
        let kb = KeybindingSettings::preset_tasty();
        assert_eq!(kb.find_conflict("quit", ""), None);
    }

    #[test]
    fn set_and_get_field_roundtrip() {
        let mut kb = KeybindingSettings::preset_tasty();
        assert!(kb.set_field("new_tab", "alt+x"));
        assert_eq!(kb.get_field("new_tab"), Some("alt+x"));
        assert!(kb.clear_field("new_tab"));
        assert_eq!(kb.get_field("new_tab"), Some(""));
    }

    #[test]
    fn set_field_unknown_returns_false() {
        let mut kb = KeybindingSettings::preset_tasty();
        assert!(!kb.set_field("nonexistent", "ctrl+x"));
        assert_eq!(kb.get_field("nonexistent"), None);
    }

    #[test]
    fn general_binding_fields_count() {
        // UI에 노출된 일반 단축키 필드: 28개.
        // 구조체에 UI 편집 가능한 필드가 추가/삭제되면 이 숫자와 GENERAL_BINDING_FIELDS를 함께 갱신해야 한다.
        assert_eq!(KeybindingSettings::GENERAL_BINDING_FIELDS.len(), 28);
    }

    #[test]
    fn all_general_fields_have_getters_and_setters() {
        let mut kb = KeybindingSettings::preset_tasty();
        for (id, _) in KeybindingSettings::GENERAL_BINDING_FIELDS {
            assert!(kb.get_field(id).is_some(), "get_field missing for {id}");
            assert!(kb.set_field(id, "x"), "set_field missing for {id}");
            assert_eq!(kb.get_field(id), Some("x"));
        }
    }

    #[test]
    fn label_key_for_returns_correct_key() {
        assert_eq!(
            KeybindingSettings::label_key_for("close_pane"),
            Some("settings.keybindings.close_pane_label")
        );
        assert_eq!(KeybindingSettings::label_key_for("nonexistent"), None);
    }
}
