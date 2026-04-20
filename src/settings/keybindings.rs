use serde::{Deserialize, Deserializer, Serialize};

/// 문자열과 Vec<String> 모두를 Vec<String>으로 역직렬화하는 필드 헬퍼.
/// 구 포맷(`new_tab = "alt+t"`)을 새 포맷(`new_tab = ["alt+t"]`)으로 자동 승격한다.
fn deserialize_binding<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StringOrVec {
        S(String),
        V(Vec<String>),
    }
    match StringOrVec::deserialize(deserializer)? {
        StringOrVec::S(s) => Ok(if s.is_empty() { Vec::new() } else { vec![s] }),
        StringOrVec::V(v) => Ok(v.into_iter().filter(|s| !s.is_empty()).collect()),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct KeybindingSettings {
    #[serde(deserialize_with = "deserialize_binding")]
    pub new_workspace: Vec<String>,
    #[serde(deserialize_with = "deserialize_binding")]
    pub new_tab: Vec<String>,
    #[serde(deserialize_with = "deserialize_binding")]
    pub split_pane_vertical: Vec<String>,
    #[serde(deserialize_with = "deserialize_binding")]
    pub split_pane_horizontal: Vec<String>,
    #[serde(deserialize_with = "deserialize_binding")]
    pub split_surface_vertical: Vec<String>,
    #[serde(deserialize_with = "deserialize_binding")]
    pub split_surface_horizontal: Vec<String>,
    #[serde(deserialize_with = "deserialize_binding")]
    pub toggle_settings: Vec<String>,
    #[serde(deserialize_with = "deserialize_binding")]
    pub toggle_notifications: Vec<String>,
    #[serde(deserialize_with = "deserialize_binding")]
    pub close_pane: Vec<String>,
    #[serde(deserialize_with = "deserialize_binding")]
    pub close_surface: Vec<String>,
    #[serde(deserialize_with = "deserialize_binding")]
    pub close_workspace: Vec<String>,
    #[serde(deserialize_with = "deserialize_binding")]
    pub focus_pane_next: Vec<String>,
    #[serde(deserialize_with = "deserialize_binding")]
    pub focus_pane_prev: Vec<String>,
    #[serde(deserialize_with = "deserialize_binding")]
    pub focus_surface_next: Vec<String>,
    #[serde(deserialize_with = "deserialize_binding")]
    pub focus_surface_prev: Vec<String>,
    /// Modifier for tab switch (number keys): "ctrl" or "alt"
    pub tab_switch_modifier: String,
    /// Modifier for workspace switch (number keys): "ctrl" or "alt"
    pub workspace_switch_modifier: String,
    /// Toggle sidebar visibility (completely hidden/shown).
    #[serde(deserialize_with = "deserialize_binding")]
    pub toggle_sidebar: Vec<String>,
    /// Toggle sidebar collapse (full/compact mode).
    #[serde(deserialize_with = "deserialize_binding")]
    pub toggle_sidebar_collapse: Vec<String>,
    /// Restore the most recently closed surface/tab/workspace.
    #[serde(deserialize_with = "deserialize_binding")]
    pub restore_closed: Vec<String>,
    /// Quit: follows close_behavior setting (ask/minimize/quit).
    #[serde(deserialize_with = "deserialize_binding")]
    pub quit: Vec<String>,
    /// Immediate quit: force exit, close everything.
    #[serde(deserialize_with = "deserialize_binding")]
    pub quit_immediate: Vec<String>,
    /// Minimize to background (park state).
    #[serde(deserialize_with = "deserialize_binding")]
    pub quit_minimize: Vec<String>,
    /// Open Markdown viewer (shows path dialog).
    #[serde(deserialize_with = "deserialize_binding")]
    pub open_markdown: Vec<String>,
    /// Open file Explorer tab.
    #[serde(deserialize_with = "deserialize_binding")]
    pub open_explorer: Vec<String>,
    /// Open Surface type convert popup.
    #[serde(deserialize_with = "deserialize_binding")]
    pub convert_surface: Vec<String>,
    /// Direct convert to Markdown (shows path dialog).
    #[serde(deserialize_with = "deserialize_binding")]
    pub convert_to_markdown: Vec<String>,
    /// Direct convert to Explorer.
    #[serde(deserialize_with = "deserialize_binding")]
    pub convert_to_explorer: Vec<String>,
    /// Open a new window.
    #[serde(deserialize_with = "deserialize_binding")]
    pub new_window: Vec<String>,
    /// Close nearest: tab → pane → workspace.
    #[serde(deserialize_with = "deserialize_binding")]
    pub close_active: Vec<String>,
    /// Focus next tab in the current pane.
    #[serde(deserialize_with = "deserialize_binding")]
    pub next_tab: Vec<String>,
    /// Focus previous tab in the current pane.
    #[serde(deserialize_with = "deserialize_binding")]
    pub prev_tab: Vec<String>,
    /// Toggle the clipboard history viewer popup.
    #[serde(deserialize_with = "deserialize_binding")]
    pub toggle_clipboard_viewer: Vec<String>,
    /// Copy selection (or inject egui Copy event) from focused surface.
    #[serde(deserialize_with = "deserialize_binding")]
    pub copy: Vec<String>,
    /// Paste clipboard content into focused terminal.
    #[serde(deserialize_with = "deserialize_binding")]
    pub paste: Vec<String>,
    /// Increase font size.
    #[serde(deserialize_with = "deserialize_binding")]
    pub zoom_in: Vec<String>,
    /// Decrease font size.
    #[serde(deserialize_with = "deserialize_binding")]
    pub zoom_out: Vec<String>,
    /// Reset font size.
    #[serde(deserialize_with = "deserialize_binding")]
    pub zoom_reset: Vec<String>,
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
        ("toggle_clipboard_viewer", "settings.keybindings.toggle_clipboard_viewer_label"),
        ("copy",                    "settings.keybindings.copy_label"),
        ("paste",                   "settings.keybindings.paste_label"),
        ("zoom_in",                 "settings.keybindings.zoom_in_label"),
        ("zoom_out",                "settings.keybindings.zoom_out_label"),
        ("zoom_reset",              "settings.keybindings.zoom_reset_label"),
    ];

    /// 필드 id로 Vec<String> 참조를 얻는다.
    pub fn get_bindings(&self, field_id: &str) -> Option<&[String]> {
        Some(match field_id {
            "new_workspace"            => self.new_workspace.as_slice(),
            "new_tab"                  => self.new_tab.as_slice(),
            "split_pane_vertical"      => self.split_pane_vertical.as_slice(),
            "split_pane_horizontal"    => self.split_pane_horizontal.as_slice(),
            "split_surface_vertical"   => self.split_surface_vertical.as_slice(),
            "split_surface_horizontal" => self.split_surface_horizontal.as_slice(),
            "toggle_settings"          => self.toggle_settings.as_slice(),
            "toggle_notifications"     => self.toggle_notifications.as_slice(),
            "close_pane"               => self.close_pane.as_slice(),
            "close_surface"            => self.close_surface.as_slice(),
            "close_workspace"          => self.close_workspace.as_slice(),
            "focus_pane_next"          => self.focus_pane_next.as_slice(),
            "focus_pane_prev"          => self.focus_pane_prev.as_slice(),
            "focus_surface_next"       => self.focus_surface_next.as_slice(),
            "focus_surface_prev"       => self.focus_surface_prev.as_slice(),
            "toggle_sidebar"           => self.toggle_sidebar.as_slice(),
            "toggle_sidebar_collapse"  => self.toggle_sidebar_collapse.as_slice(),
            "restore_closed"           => self.restore_closed.as_slice(),
            "quit"                     => self.quit.as_slice(),
            "quit_immediate"           => self.quit_immediate.as_slice(),
            "quit_minimize"            => self.quit_minimize.as_slice(),
            "open_markdown"            => self.open_markdown.as_slice(),
            "open_explorer"            => self.open_explorer.as_slice(),
            "convert_surface"          => self.convert_surface.as_slice(),
            "convert_to_markdown"      => self.convert_to_markdown.as_slice(),
            "convert_to_explorer"      => self.convert_to_explorer.as_slice(),
            "new_window"               => self.new_window.as_slice(),
            "close_active"             => self.close_active.as_slice(),
            "next_tab"                 => self.next_tab.as_slice(),
            "prev_tab"                 => self.prev_tab.as_slice(),
            "toggle_clipboard_viewer"  => self.toggle_clipboard_viewer.as_slice(),
            "copy"                     => self.copy.as_slice(),
            "paste"                    => self.paste.as_slice(),
            "zoom_in"                  => self.zoom_in.as_slice(),
            "zoom_out"                 => self.zoom_out.as_slice(),
            "zoom_reset"               => self.zoom_reset.as_slice(),
            _ => return None,
        })
    }

    fn get_bindings_mut(&mut self, field_id: &str) -> Option<&mut Vec<String>> {
        Some(match field_id {
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
            "toggle_clipboard_viewer"  => &mut self.toggle_clipboard_viewer,
            "copy"                     => &mut self.copy,
            "paste"                    => &mut self.paste,
            "zoom_in"                  => &mut self.zoom_in,
            "zoom_out"                 => &mut self.zoom_out,
            "zoom_reset"               => &mut self.zoom_reset,
            _ => return None,
        })
    }

    /// 편의: 첫 번째 바인딩을 반환. 없으면 빈 문자열 슬라이스.
    pub fn get_field(&self, field_id: &str) -> Option<&str> {
        self.get_bindings(field_id)
            .map(|v| v.first().map(|s| s.as_str()).unwrap_or(""))
    }

    /// 편의: 단일 값으로 덮어씀. 빈 문자열이면 바인딩 전체 제거.
    pub fn set_field(&mut self, field_id: &str, value: &str) -> bool {
        let Some(vec) = self.get_bindings_mut(field_id) else { return false; };
        vec.clear();
        if !value.is_empty() {
            vec.push(value.to_string());
        }
        true
    }

    pub fn clear_field(&mut self, field_id: &str) -> bool {
        let Some(vec) = self.get_bindings_mut(field_id) else { return false; };
        vec.clear();
        true
    }

    /// field의 바인딩 목록에 combo를 추가. 이미 있으면 추가하지 않고 false 반환.
    /// combo가 빈 문자열이면 false.
    pub fn add_binding(&mut self, field_id: &str, combo: String) -> bool {
        if combo.is_empty() {
            return false;
        }
        let Some(vec) = self.get_bindings_mut(field_id) else { return false; };
        if vec.iter().any(|b| b == &combo) {
            return false;
        }
        vec.push(combo);
        true
    }

    /// field의 idx 번째 바인딩을 제거.
    pub fn remove_binding(&mut self, field_id: &str, idx: usize) -> bool {
        let Some(vec) = self.get_bindings_mut(field_id) else { return false; };
        if idx >= vec.len() {
            return false;
        }
        vec.remove(idx);
        true
    }

    /// field의 idx 번째 바인딩을 combo로 교체. idx가 len()이면 새로 push.
    pub fn replace_binding_at(&mut self, field_id: &str, idx: usize, combo: String) -> bool {
        if combo.is_empty() {
            return false;
        }
        let Some(vec) = self.get_bindings_mut(field_id) else { return false; };
        if idx == vec.len() {
            // 이미 같은 combo가 있으면 중복 추가 금지
            if vec.iter().any(|b| b == &combo) {
                return false;
            }
            vec.push(combo);
            true
        } else if idx < vec.len() {
            vec[idx] = combo;
            true
        } else {
            false
        }
    }

    /// `combo`와 같은 값을 가진 **다른 필드**의 (field_id, idx)를 반환.
    /// 빈 조합은 항상 None. 자기 자신(field_id 일치)은 제외.
    pub fn find_conflict(&self, field_id: &str, combo: &str) -> Option<(&'static str, usize)> {
        if combo.is_empty() {
            return None;
        }
        for (id, _label) in Self::GENERAL_BINDING_FIELDS {
            if *id == field_id {
                continue;
            }
            if let Some(bindings) = self.get_bindings(id) {
                if let Some(idx) = bindings.iter().position(|b| b == combo) {
                    return Some((id, idx));
                }
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
    ///
    /// 주의: `split('+')`은 쓸 수 없다. `"ctrl++"`(Ctrl+`+키`) 같은 바인딩에서 구분자
    /// `+`와 키 이름 `+`를 구분하지 못하기 때문. 왼쪽부터 모디파이어 프리픽스를 하나씩
    /// 떼어내고, 남은 부분을 통째로 키 토큰으로 본다.
    pub fn format_display(binding: &str) -> String {
        if binding.is_empty() {
            return String::new();
        }

        let mut parts: Vec<String> = Vec::new();
        let mut rest = binding;
        let (mut ctrl, mut shift, mut alt) = (false, false, false);
        loop {
            let lower = rest.to_ascii_lowercase();
            if !ctrl && lower.starts_with("ctrl+") {
                ctrl = true;
                parts.push("Ctrl".into());
                rest = &rest[5..];
            } else if !shift && lower.starts_with("shift+") {
                shift = true;
                parts.push("Shift".into());
                rest = &rest[6..];
            } else if !alt && lower.starts_with("alt+") {
                alt = true;
                parts.push("Alt".into());
                rest = &rest[4..];
            } else {
                break;
            }
        }

        if !rest.is_empty() {
            let key_display = match rest.to_ascii_lowercase().as_str() {
                "plus" => "+".into(),
                "minus" => "-".into(),
                "equals" => "=".into(),
                _ => {
                    let mut chars = rest.chars();
                    match chars.next() {
                        Some(first) => format!("{}{}", first.to_uppercase(), chars.as_str()),
                        None => String::new(),
                    }
                }
            };
            parts.push(key_display);
        }

        parts.join("+")
    }
}

impl Default for KeybindingSettings {
    fn default() -> Self {
        Self::preset_tasty()
    }
}

impl KeybindingSettings {
    /// Tasty preset (default). All platform copy/paste/zoom bindings combined.
    pub fn preset_tasty() -> Self {
        Self {
            new_workspace: vec!["alt+n".into()],
            new_tab: vec!["alt+t".into()],
            split_pane_vertical: vec!["alt+e".into()],
            split_pane_horizontal: vec!["alt+shift+e".into()],
            split_surface_vertical: vec!["alt+d".into()],
            split_surface_horizontal: vec!["alt+shift+d".into()],
            toggle_settings: vec!["ctrl+,".into()],
            toggle_notifications: vec!["ctrl+shift+i".into()],
            close_pane: vec!["ctrl+shift+w".into()],
            close_surface: Vec::new(),
            close_workspace: vec!["alt+shift+w".into()],
            focus_pane_next: vec!["ctrl+]".into()],
            focus_pane_prev: vec!["ctrl+[".into()],
            focus_surface_next: vec!["alt+]".into()],
            focus_surface_prev: vec!["alt+[".into()],
            tab_switch_modifier: "ctrl".to_string(),
            workspace_switch_modifier: "alt".to_string(),
            toggle_sidebar: vec!["ctrl+shift+b".into()],
            toggle_sidebar_collapse: vec!["ctrl+b".into()],
            restore_closed: vec!["ctrl+shift+t".into()],
            quit: Vec::new(),
            quit_immediate: Vec::new(),
            quit_minimize: Vec::new(),
            open_markdown: Vec::new(),
            open_explorer: Vec::new(),
            convert_surface: vec!["alt+'".into()],
            convert_to_markdown: Vec::new(),
            convert_to_explorer: Vec::new(),
            new_window: vec!["alt+shift+n".into()],
            close_active: vec!["ctrl+w".into()],
            next_tab: Vec::new(),
            prev_tab: Vec::new(),
            toggle_clipboard_viewer: vec!["ctrl+shift+h".into()],
            copy: vec!["ctrl+c".into(), "alt+c".into(), "ctrl+shift+c".into()],
            paste: vec!["ctrl+v".into(), "alt+v".into(), "ctrl+shift+v".into()],
            zoom_in: vec!["ctrl+=".into(), "ctrl++".into(), "alt+=".into(), "alt++".into()],
            zoom_out: vec!["ctrl+-".into(), "alt+-".into()],
            zoom_reset: vec!["ctrl+0".into(), "alt+0".into()],
        }
    }

    /// Mac preset. ⌘ (alt) centric, following iTerm2 / Terminal.app conventions.
    pub fn preset_mac() -> Self {
        Self {
            new_workspace: vec!["alt+n".into()],
            new_tab: vec!["alt+t".into()],
            split_pane_vertical: vec!["alt+e".into()],
            split_pane_horizontal: vec!["alt+shift+e".into()],
            split_surface_vertical: vec!["alt+d".into()],
            split_surface_horizontal: vec!["alt+shift+d".into()],
            toggle_settings: vec!["alt+,".into()],
            toggle_notifications: vec!["alt+shift+i".into()],
            close_pane: vec!["alt+shift+w".into()],
            close_surface: Vec::new(),
            close_workspace: Vec::new(),
            focus_pane_next: vec!["ctrl+]".into()],
            focus_pane_prev: vec!["ctrl+[".into()],
            focus_surface_next: vec!["alt+]".into()],
            focus_surface_prev: vec!["alt+[".into()],
            tab_switch_modifier: "ctrl".to_string(),
            workspace_switch_modifier: "alt".to_string(),
            toggle_sidebar: vec!["alt+shift+b".into()],
            toggle_sidebar_collapse: vec!["alt+b".into()],
            restore_closed: vec!["ctrl+shift+t".into()],
            quit: vec!["alt+q".into()],
            quit_immediate: Vec::new(),
            quit_minimize: vec!["alt+m".into()],
            open_markdown: Vec::new(),
            open_explorer: Vec::new(),
            convert_surface: vec!["alt+'".into()],
            convert_to_markdown: Vec::new(),
            convert_to_explorer: Vec::new(),
            new_window: vec!["alt+shift+n".into()],
            close_active: vec!["alt+w".into()],
            next_tab: Vec::new(),
            prev_tab: Vec::new(),
            toggle_clipboard_viewer: vec!["alt+shift+h".into()],
            copy: vec!["alt+c".into()],
            paste: vec!["alt+v".into()],
            zoom_in: vec!["alt+=".into(), "alt++".into()],
            zoom_out: vec!["alt+-".into()],
            zoom_reset: vec!["alt+0".into()],
        }
    }

    /// Windows preset. Ctrl+Shift centric, following Windows Terminal conventions.
    pub fn preset_windows() -> Self {
        Self {
            new_workspace: vec!["alt+n".into()],
            new_tab: vec!["alt+t".into()],
            split_pane_vertical: vec!["alt+shift+e".into()],
            split_pane_horizontal: vec!["alt+shift+d".into()],
            split_surface_vertical: vec!["alt+d".into()],
            split_surface_horizontal: vec!["alt+e".into()],
            toggle_settings: vec!["ctrl+,".into()],
            toggle_notifications: vec!["ctrl+shift+i".into()],
            close_pane: vec!["ctrl+shift+w".into()],
            close_surface: Vec::new(),
            close_workspace: vec!["alt+shift+w".into()],
            focus_pane_next: vec!["ctrl+]".into()],
            focus_pane_prev: vec!["ctrl+[".into()],
            focus_surface_next: vec!["alt+]".into()],
            focus_surface_prev: vec!["alt+[".into()],
            tab_switch_modifier: "ctrl".to_string(),
            workspace_switch_modifier: "alt".to_string(),
            toggle_sidebar: vec!["ctrl+shift+b".into()],
            toggle_sidebar_collapse: vec!["ctrl+b".into()],
            restore_closed: vec!["ctrl+shift+t".into()],
            quit: Vec::new(),
            quit_immediate: Vec::new(),
            quit_minimize: Vec::new(),
            open_markdown: Vec::new(),
            open_explorer: Vec::new(),
            convert_surface: vec!["alt+'".into()],
            convert_to_markdown: Vec::new(),
            convert_to_explorer: Vec::new(),
            new_window: vec!["ctrl+shift+n".into()],
            close_active: vec!["ctrl+w".into()],
            next_tab: Vec::new(),
            prev_tab: Vec::new(),
            toggle_clipboard_viewer: vec!["ctrl+shift+h".into()],
            copy: vec!["ctrl+c".into()],
            paste: vec!["ctrl+v".into()],
            zoom_in: vec!["ctrl+=".into(), "ctrl++".into()],
            zoom_out: vec!["ctrl+-".into()],
            zoom_reset: vec!["ctrl+0".into()],
        }
    }

    /// Linux preset. Ctrl+Shift centric, following GNOME Terminal conventions.
    pub fn preset_linux() -> Self {
        Self {
            new_workspace: vec!["alt+n".into()],
            new_tab: vec!["alt+t".into()],
            split_pane_vertical: vec!["alt+shift+e".into()],
            split_pane_horizontal: vec!["alt+shift+d".into()],
            split_surface_vertical: vec!["alt+d".into()],
            split_surface_horizontal: vec!["alt+e".into()],
            toggle_settings: vec!["ctrl+,".into()],
            toggle_notifications: vec!["ctrl+shift+i".into()],
            close_pane: vec!["ctrl+shift+w".into()],
            close_surface: Vec::new(),
            close_workspace: vec!["alt+shift+w".into()],
            focus_pane_next: vec!["ctrl+]".into()],
            focus_pane_prev: vec!["ctrl+[".into()],
            focus_surface_next: vec!["alt+]".into()],
            focus_surface_prev: vec!["alt+[".into()],
            tab_switch_modifier: "ctrl".to_string(),
            workspace_switch_modifier: "alt".to_string(),
            toggle_sidebar: vec!["ctrl+shift+b".into()],
            toggle_sidebar_collapse: vec!["ctrl+b".into()],
            restore_closed: vec!["ctrl+shift+t".into()],
            quit: vec!["ctrl+q".into()],
            quit_immediate: Vec::new(),
            quit_minimize: Vec::new(),
            open_markdown: Vec::new(),
            open_explorer: Vec::new(),
            convert_surface: vec!["alt+'".into()],
            convert_to_markdown: Vec::new(),
            convert_to_explorer: Vec::new(),
            new_window: vec!["ctrl+shift+n".into()],
            close_active: vec!["ctrl+w".into()],
            next_tab: Vec::new(),
            prev_tab: Vec::new(),
            toggle_clipboard_viewer: vec!["ctrl+shift+h".into()],
            copy: vec!["ctrl+shift+c".into()],
            paste: vec!["ctrl+shift+v".into()],
            zoom_in: vec!["ctrl+=".into(), "ctrl++".into()],
            zoom_out: vec!["ctrl+-".into()],
            zoom_reset: vec!["ctrl+0".into()],
        }
    }

    /// List available preset names.
    pub fn preset_names() -> &'static [&'static str] {
        &["Tasty", "Mac", "Windows", "Linux"]
    }

    /// 이름으로 프리셋의 원본 인스턴스를 얻는다. 미리보기/적용 공통 소스.
    pub fn preset_by_name(name: &str) -> Option<Self> {
        match name {
            "Tasty" => Some(Self::preset_tasty()),
            "Mac" => Some(Self::preset_mac()),
            "Windows" => Some(Self::preset_windows()),
            "Linux" => Some(Self::preset_linux()),
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
                by_name.get_bindings(id),
                direct.get_bindings(id),
                "field {id} mismatch"
            );
        }
        assert!(KeybindingSettings::preset_by_name("Unknown").is_none());
    }

    #[test]
    fn preset_by_name_matches_preset_mac() {
        let by_name = KeybindingSettings::preset_by_name("Mac").unwrap();
        let direct = KeybindingSettings::preset_mac();
        for (id, _) in KeybindingSettings::GENERAL_BINDING_FIELDS {
            assert_eq!(
                by_name.get_bindings(id),
                direct.get_bindings(id),
                "field {id} mismatch"
            );
        }
    }

    #[test]
    fn preset_by_name_matches_preset_windows() {
        let by_name = KeybindingSettings::preset_by_name("Windows").unwrap();
        let direct = KeybindingSettings::preset_windows();
        for (id, _) in KeybindingSettings::GENERAL_BINDING_FIELDS {
            assert_eq!(
                by_name.get_bindings(id),
                direct.get_bindings(id),
                "field {id} mismatch"
            );
        }
    }

    #[test]
    fn preset_by_name_matches_preset_linux() {
        let by_name = KeybindingSettings::preset_by_name("Linux").unwrap();
        let direct = KeybindingSettings::preset_linux();
        for (id, _) in KeybindingSettings::GENERAL_BINDING_FIELDS {
            assert_eq!(
                by_name.get_bindings(id),
                direct.get_bindings(id),
                "field {id} mismatch"
            );
        }
    }

    #[test]
    fn preset_names_lists_all_four() {
        let names = KeybindingSettings::preset_names();
        assert_eq!(names, &["Tasty", "Mac", "Windows", "Linux"]);
    }

    /// 각 프리셋 내부에 바인딩 충돌이 없는지 검증.
    #[test]
    fn no_conflicts_within_presets() {
        for name in KeybindingSettings::preset_names() {
            let kb = KeybindingSettings::preset_by_name(name).unwrap();
            for (id, _) in KeybindingSettings::GENERAL_BINDING_FIELDS {
                if let Some(bindings) = kb.get_bindings(id) {
                    for combo in bindings {
                        if combo.is_empty() {
                            continue;
                        }
                        let conflict = kb.find_conflict(id, combo);
                        assert_eq!(
                            conflict, None,
                            "preset '{name}': field '{id}' combo '{combo}' conflicts with {:?}",
                            conflict
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn find_conflict_detects_duplicate_across_fields() {
        let kb = KeybindingSettings::preset_tasty();
        assert_eq!(
            kb.find_conflict("toggle_settings", "ctrl+shift+w"),
            Some(("close_pane", 0))
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
    fn set_and_get_field_roundtrip() {
        let mut kb = KeybindingSettings::preset_tasty();
        assert!(kb.set_field("new_tab", "alt+x"));
        assert_eq!(kb.get_field("new_tab"), Some("alt+x"));
        assert!(kb.clear_field("new_tab"));
        assert_eq!(kb.get_field("new_tab"), Some(""));
    }

    #[test]
    fn add_remove_binding_roundtrip() {
        let mut kb = KeybindingSettings::preset_tasty();
        assert!(kb.clear_field("copy"));
        assert!(kb.add_binding("copy", "ctrl+c".into()));
        assert!(kb.add_binding("copy", "ctrl+shift+c".into()));
        // 중복 추가는 실패.
        assert!(!kb.add_binding("copy", "ctrl+c".into()));
        assert_eq!(kb.get_bindings("copy"), Some(&["ctrl+c".to_string(), "ctrl+shift+c".to_string()][..]));
        assert!(kb.remove_binding("copy", 0));
        assert_eq!(kb.get_bindings("copy"), Some(&["ctrl+shift+c".to_string()][..]));
        // 범위 밖 idx는 실패.
        assert!(!kb.remove_binding("copy", 5));
    }

    #[test]
    fn add_binding_rejects_empty() {
        let mut kb = KeybindingSettings::preset_tasty();
        assert!(!kb.add_binding("new_tab", String::new()));
    }

    #[test]
    fn set_field_unknown_returns_false() {
        let mut kb = KeybindingSettings::preset_tasty();
        assert!(!kb.set_field("nonexistent", "ctrl+x"));
        assert_eq!(kb.get_field("nonexistent"), None);
    }

    #[test]
    fn general_binding_fields_count() {
        // UI에 노출된 일반 단축키 필드: 34개. (기존 29 + copy/paste/zoom_in/zoom_out/zoom_reset 5)
        assert_eq!(KeybindingSettings::GENERAL_BINDING_FIELDS.len(), 34);
    }

    #[test]
    fn all_general_fields_have_getters_and_setters() {
        let mut kb = KeybindingSettings::preset_tasty();
        for (id, _) in KeybindingSettings::GENERAL_BINDING_FIELDS {
            assert!(kb.get_bindings(id).is_some(), "get_bindings missing for {id}");
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
        assert_eq!(
            KeybindingSettings::label_key_for("copy"),
            Some("settings.keybindings.copy_label")
        );
        assert_eq!(KeybindingSettings::label_key_for("nonexistent"), None);
    }

    /// 구 포맷(단일 String) TOML이 Vec<String>으로 자동 승격되는지 확인.
    #[test]
    fn legacy_string_format_deserializes_as_single_element_vec() {
        let toml_str = r#"
new_tab = "alt+x"
close_pane = ""
copy = ["ctrl+c", "ctrl+shift+c"]
"#;
        let kb: KeybindingSettings = toml::from_str(toml_str).unwrap();
        assert_eq!(kb.new_tab, vec!["alt+x".to_string()]);
        // 빈 문자열은 빈 Vec으로.
        assert!(kb.close_pane.is_empty());
        assert_eq!(kb.copy, vec!["ctrl+c".to_string(), "ctrl+shift+c".to_string()]);
    }

    // ── format_display: `+` 구분자/키 충돌 해소 ───────────────────────

    #[test]
    fn format_display_basic() {
        assert_eq!(
            KeybindingSettings::format_display("ctrl+shift+n"),
            "Ctrl+Shift+N"
        );
    }

    #[test]
    fn format_display_ctrl_plus_key() {
        // "ctrl++"는 Ctrl + `+` 키. 표시상 "Ctrl++"여야 한다.
        assert_eq!(KeybindingSettings::format_display("ctrl++"), "Ctrl++");
    }

    #[test]
    fn format_display_symbol_aliases() {
        assert_eq!(KeybindingSettings::format_display("ctrl+plus"), "Ctrl++");
        assert_eq!(KeybindingSettings::format_display("ctrl+minus"), "Ctrl+-");
        assert_eq!(KeybindingSettings::format_display("ctrl+equals"), "Ctrl+=");
    }

    #[test]
    fn format_display_empty_and_minus() {
        assert_eq!(KeybindingSettings::format_display(""), "");
        assert_eq!(KeybindingSettings::format_display("ctrl+-"), "Ctrl+-");
        assert_eq!(KeybindingSettings::format_display("ctrl+="), "Ctrl+=");
    }
}
