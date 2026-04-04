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
}

impl KeybindingSettings {
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
        }
    }

    /// List available preset names.
    pub fn preset_names() -> &'static [&'static str] {
        &["Tasty"]
    }

    /// Apply a preset by name. Returns true if found.
    pub fn apply_preset(&mut self, name: &str) -> bool {
        match name {
            "Tasty" => {
                *self = Self::preset_tasty();
                true
            }
            _ => false,
        }
    }
}
