//! `KeybindingSettings` — CRUD 메서드 (binding 추가/제거/충돌 검사 등).

use std::collections::HashSet;

use super::KeybindingSettings;

impl KeybindingSettings {
    /// 일반 단축키 필드 전체 목록 (modifier 필드 제외).
    /// 중복 검사 및 field_id ↔ 라벨 매핑에 사용.
    pub const GENERAL_BINDING_FIELDS: &'static [(&'static str, &'static str)] = &[
        ("new_workspace", "settings.keybindings.new_workspace_label"),
        ("new_tab", "settings.keybindings.new_tab_label"),
        (
            "split_pane_vertical",
            "settings.keybindings.split_pane_vertical_label",
        ),
        (
            "split_pane_horizontal",
            "settings.keybindings.split_pane_horizontal_label",
        ),
        (
            "split_surface_vertical",
            "settings.keybindings.split_surface_vertical_label",
        ),
        (
            "split_surface_horizontal",
            "settings.keybindings.split_surface_horizontal_label",
        ),
        (
            "toggle_settings",
            "settings.keybindings.toggle_settings_label",
        ),
        (
            "toggle_notifications",
            "settings.keybindings.toggle_notifications_label",
        ),
        ("close_pane", "settings.keybindings.close_pane_label"),
        ("close_surface", "settings.keybindings.close_surface_label"),
        (
            "close_workspace",
            "settings.keybindings.close_workspace_label",
        ),
        (
            "focus_pane_next",
            "settings.keybindings.focus_pane_next_label",
        ),
        (
            "focus_pane_prev",
            "settings.keybindings.focus_pane_prev_label",
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
            "restore_closed",
            "settings.keybindings.restore_closed_label",
        ),
        ("quit", "settings.keybindings.quit_label"),
        (
            "quit_immediate",
            "settings.keybindings.quit_immediate_label",
        ),
        ("quit_minimize", "settings.keybindings.quit_minimize_label"),
        ("open_markdown", "settings.keybindings.open_markdown_label"),
        ("open_explorer", "settings.keybindings.open_explorer_label"),
        (
            "convert_surface",
            "settings.keybindings.convert_surface_label",
        ),
        (
            "convert_to_markdown",
            "settings.keybindings.convert_to_markdown_label",
        ),
        (
            "convert_to_explorer",
            "settings.keybindings.convert_to_explorer_label",
        ),
        ("new_window", "settings.keybindings.new_window_label"),
        ("close_active", "settings.keybindings.close_active_label"),
        ("next_tab", "settings.keybindings.next_tab_label"),
        ("prev_tab", "settings.keybindings.prev_tab_label"),
        (
            "toggle_clipboard_viewer",
            "settings.keybindings.toggle_clipboard_viewer_label",
        ),
        ("find", "settings.keybindings.find_label"),
        ("copy", "settings.keybindings.copy_label"),
        ("copy_path", "settings.keybindings.copy_path_label"),
        ("cut", "settings.keybindings.cut_label"),
        ("select_all", "settings.keybindings.select_all_label"),
        ("paste", "settings.keybindings.paste_label"),
        ("zoom_in", "settings.keybindings.zoom_in_label"),
        ("zoom_out", "settings.keybindings.zoom_out_label"),
        ("zoom_reset", "settings.keybindings.zoom_reset_label"),
        ("rename_tab", "settings.keybindings.rename_tab_label"),
        (
            "rename_workspace",
            "settings.keybindings.rename_workspace_label",
        ),
        (
            "rename_workspace_subtitle",
            "settings.keybindings.rename_workspace_subtitle_label",
        ),
        ("image_undo", "settings.keybindings.image_undo_label"),
        ("image_redo", "settings.keybindings.image_redo_label"),
        (
            "toggle_command_palette",
            "settings.keybindings.toggle_command_palette_label",
        ),
        (
            "apply_workspace_preset",
            "settings.keybindings.apply_workspace_preset_label",
        ),
        (
            "apply_tab_preset",
            "settings.keybindings.apply_tab_preset_label",
        ),
        (
            "apply_pane_preset",
            "settings.keybindings.apply_pane_preset_label",
        ),
        (
            "minimize_window",
            "settings.keybindings.minimize_window_label",
        ),
        (
            "maximize_window",
            "settings.keybindings.maximize_window_label",
        ),
        ("close_window", "settings.keybindings.close_window_label"),
    ];

    /// 필드 id로 Vec<String> 참조를 얻는다.
    pub fn get_bindings(&self, field_id: &str) -> Option<&[String]> {
        Some(match field_id {
            "new_workspace" => self.new_workspace.as_slice(),
            "new_tab" => self.new_tab.as_slice(),
            "split_pane_vertical" => self.split_pane_vertical.as_slice(),
            "split_pane_horizontal" => self.split_pane_horizontal.as_slice(),
            "split_surface_vertical" => self.split_surface_vertical.as_slice(),
            "split_surface_horizontal" => self.split_surface_horizontal.as_slice(),
            "toggle_settings" => self.toggle_settings.as_slice(),
            "toggle_notifications" => self.toggle_notifications.as_slice(),
            "close_pane" => self.close_pane.as_slice(),
            "close_surface" => self.close_surface.as_slice(),
            "close_workspace" => self.close_workspace.as_slice(),
            "focus_pane_next" => self.focus_pane_next.as_slice(),
            "focus_pane_prev" => self.focus_pane_prev.as_slice(),
            "focus_surface_next" => self.focus_surface_next.as_slice(),
            "focus_surface_prev" => self.focus_surface_prev.as_slice(),
            "toggle_sidebar" => self.toggle_sidebar.as_slice(),
            "toggle_sidebar_collapse" => self.toggle_sidebar_collapse.as_slice(),
            "restore_closed" => self.restore_closed.as_slice(),
            "quit" => self.quit.as_slice(),
            "quit_immediate" => self.quit_immediate.as_slice(),
            "quit_minimize" => self.quit_minimize.as_slice(),
            "open_markdown" => self.open_markdown.as_slice(),
            "open_explorer" => self.open_explorer.as_slice(),
            "convert_surface" => self.convert_surface.as_slice(),
            "convert_to_markdown" => self.convert_to_markdown.as_slice(),
            "convert_to_explorer" => self.convert_to_explorer.as_slice(),
            "new_window" => self.new_window.as_slice(),
            "close_active" => self.close_active.as_slice(),
            "next_tab" => self.next_tab.as_slice(),
            "prev_tab" => self.prev_tab.as_slice(),
            "toggle_clipboard_viewer" => self.toggle_clipboard_viewer.as_slice(),
            "find" => self.find.as_slice(),
            "copy" => self.copy.as_slice(),
            "copy_path" => self.copy_path.as_slice(),
            "cut" => self.cut.as_slice(),
            "select_all" => self.select_all.as_slice(),
            "paste" => self.paste.as_slice(),
            "zoom_in" => self.zoom_in.as_slice(),
            "zoom_out" => self.zoom_out.as_slice(),
            "zoom_reset" => self.zoom_reset.as_slice(),
            "rename_tab" => self.rename_tab.as_slice(),
            "rename_workspace" => self.rename_workspace.as_slice(),
            "rename_workspace_subtitle" => self.rename_workspace_subtitle.as_slice(),
            "image_undo" => self.image_undo.as_slice(),
            "image_redo" => self.image_redo.as_slice(),
            "toggle_command_palette" => self.toggle_command_palette.as_slice(),
            "apply_workspace_preset" => self.apply_workspace_preset.as_slice(),
            "apply_tab_preset" => self.apply_tab_preset.as_slice(),
            "apply_pane_preset" => self.apply_pane_preset.as_slice(),
            "minimize_window" => self.minimize_window.as_slice(),
            "maximize_window" => self.maximize_window.as_slice(),
            "close_window" => self.close_window.as_slice(),
            _ => return None,
        })
    }

    fn get_bindings_mut(&mut self, field_id: &str) -> Option<&mut Vec<String>> {
        Some(match field_id {
            "new_workspace" => &mut self.new_workspace,
            "new_tab" => &mut self.new_tab,
            "split_pane_vertical" => &mut self.split_pane_vertical,
            "split_pane_horizontal" => &mut self.split_pane_horizontal,
            "split_surface_vertical" => &mut self.split_surface_vertical,
            "split_surface_horizontal" => &mut self.split_surface_horizontal,
            "toggle_settings" => &mut self.toggle_settings,
            "toggle_notifications" => &mut self.toggle_notifications,
            "close_pane" => &mut self.close_pane,
            "close_surface" => &mut self.close_surface,
            "close_workspace" => &mut self.close_workspace,
            "focus_pane_next" => &mut self.focus_pane_next,
            "focus_pane_prev" => &mut self.focus_pane_prev,
            "focus_surface_next" => &mut self.focus_surface_next,
            "focus_surface_prev" => &mut self.focus_surface_prev,
            "toggle_sidebar" => &mut self.toggle_sidebar,
            "toggle_sidebar_collapse" => &mut self.toggle_sidebar_collapse,
            "restore_closed" => &mut self.restore_closed,
            "quit" => &mut self.quit,
            "quit_immediate" => &mut self.quit_immediate,
            "quit_minimize" => &mut self.quit_minimize,
            "open_markdown" => &mut self.open_markdown,
            "open_explorer" => &mut self.open_explorer,
            "convert_surface" => &mut self.convert_surface,
            "convert_to_markdown" => &mut self.convert_to_markdown,
            "convert_to_explorer" => &mut self.convert_to_explorer,
            "new_window" => &mut self.new_window,
            "close_active" => &mut self.close_active,
            "next_tab" => &mut self.next_tab,
            "prev_tab" => &mut self.prev_tab,
            "toggle_clipboard_viewer" => &mut self.toggle_clipboard_viewer,
            "find" => &mut self.find,
            "copy" => &mut self.copy,
            "copy_path" => &mut self.copy_path,
            "cut" => &mut self.cut,
            "select_all" => &mut self.select_all,
            "paste" => &mut self.paste,
            "zoom_in" => &mut self.zoom_in,
            "zoom_out" => &mut self.zoom_out,
            "zoom_reset" => &mut self.zoom_reset,
            "rename_tab" => &mut self.rename_tab,
            "rename_workspace" => &mut self.rename_workspace,
            "rename_workspace_subtitle" => &mut self.rename_workspace_subtitle,
            "image_undo" => &mut self.image_undo,
            "image_redo" => &mut self.image_redo,
            "toggle_command_palette" => &mut self.toggle_command_palette,
            "apply_workspace_preset" => &mut self.apply_workspace_preset,
            "apply_tab_preset" => &mut self.apply_tab_preset,
            "apply_pane_preset" => &mut self.apply_pane_preset,
            "minimize_window" => &mut self.minimize_window,
            "maximize_window" => &mut self.maximize_window,
            "close_window" => &mut self.close_window,
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
        let Some(vec) = self.get_bindings_mut(field_id) else {
            return false;
        };
        vec.clear();
        if !value.is_empty() {
            vec.push(value.to_string());
        }
        true
    }

    pub fn clear_field(&mut self, field_id: &str) -> bool {
        let Some(vec) = self.get_bindings_mut(field_id) else {
            return false;
        };
        vec.clear();
        true
    }

    /// field의 바인딩 목록에 combo를 추가. 이미 있으면 추가하지 않고 false 반환.
    /// combo가 빈 문자열이면 false.
    pub fn add_binding(&mut self, field_id: &str, combo: String) -> bool {
        if combo.is_empty() {
            return false;
        }
        let Some(vec) = self.get_bindings_mut(field_id) else {
            return false;
        };
        if vec.iter().any(|b| b == &combo) {
            return false;
        }
        vec.push(combo);
        true
    }

    /// field의 idx 번째 바인딩을 제거.
    pub fn remove_binding(&mut self, field_id: &str, idx: usize) -> bool {
        let Some(vec) = self.get_bindings_mut(field_id) else {
            return false;
        };
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
        let Some(vec) = self.get_bindings_mut(field_id) else {
            return false;
        };
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
            if let Some(bindings) = self.get_bindings(id)
                && let Some(idx) = bindings.iter().position(|b| b == combo)
            {
                return Some((id, idx));
            }
        }
        None
    }

    /// 기본값으로 채워진 필드에서 사용자 설정 필드와 충돌하는 바인딩을 제거한다.
    ///
    /// `existing_keys`: TOML에 실제로 존재했던 keybindings 키 목록.
    /// existing_keys에 없는 필드(= 기본값으로 채워진 필드)의 바인딩 중,
    /// 다른 필드와 중복되는 것을 제거한다.
    pub fn remove_conflicts_from_defaults(&mut self, existing_keys: &HashSet<String>) {
        // 먼저 모든 사용자 설정 바인딩을 수집
        let mut user_combos: HashSet<String> = HashSet::new();
        for (field_id, _) in Self::GENERAL_BINDING_FIELDS {
            if existing_keys.contains(*field_id)
                && let Some(bindings) = self.get_bindings(field_id)
            {
                for combo in bindings {
                    if !combo.is_empty() {
                        user_combos.insert(combo.clone());
                    }
                }
            }
        }

        // 기본값 필드에서 사용자 바인딩과 충돌하는 combo 제거
        for (field_id, _) in Self::GENERAL_BINDING_FIELDS {
            if existing_keys.contains(*field_id) {
                continue;
            }
            if let Some(vec) = self.get_bindings_mut(field_id) {
                let before = vec.len();
                vec.retain(|combo| !user_combos.contains(combo));
                let removed = before - vec.len();
                if removed > 0 {
                    tracing::info!(
                        "removed {removed} conflicting default binding(s) from '{field_id}'"
                    );
                }
            }
        }
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
    /// [`Self::format_display_parts`] 의 토큰을 `+` 로 join 한 단일 문자열.
    pub fn format_display(binding: &str) -> String {
        Self::format_display_parts(binding).join("+")
    }

    /// Tokenize a binding into display키캡 단위 (e.g. "ctrl+shift+n" → ["Ctrl","Shift","N"]).
    ///
    /// 키캡 분해 렌더(명령 팔레트 Kbd 등)가 `+` 구분자 모호성 없이 토큰을 받도록 하는
    /// 정식 경로. 반환 문자열을 `split('+')` 하면 `"ctrl++"`(Ctrl+`+키`) 같은 케이스가
    /// 깨지므로, 표시 문자열 대신 **이 함수**를 써야 한다.
    ///
    /// 주의: `split('+')`은 쓸 수 없다. `"ctrl++"`(Ctrl+`+키`) 같은 바인딩에서 구분자
    /// `+`와 키 이름 `+`를 구분하지 못하기 때문. 왼쪽부터 모디파이어 프리픽스를 하나씩
    /// 떼어내고, 남은 부분을 통째로 키 토큰으로 본다.
    pub fn format_display_parts(binding: &str) -> Vec<String> {
        if binding.is_empty() {
            return Vec::new();
        }

        let mut parts: Vec<String> = Vec::new();
        let mut rest = binding;
        let (mut ctrl, mut shift, mut alt, mut option) = (false, false, false, false);
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
            } else if !option && lower.starts_with("option+") {
                option = true;
                parts.push("Option".into());
                rest = &rest[7..];
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

        parts
    }
}
