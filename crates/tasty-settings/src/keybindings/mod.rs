
use serde::{Deserialize, Serialize};

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
    pub close_pane: Vec<String>,
    pub close_surface: Vec<String>,
    pub close_workspace: Vec<String>,
    pub focus_pane_next: Vec<String>,
    pub focus_pane_prev: Vec<String>,
    pub focus_surface_next: Vec<String>,
    pub focus_surface_prev: Vec<String>,
    /// Modifier for tab switch (number keys): "ctrl" or "alt"
    pub tab_switch_modifier: String,
    /// Modifier for workspace switch (number keys): "ctrl" or "alt"
    pub workspace_switch_modifier: String,
    /// Toggle sidebar visibility (completely hidden/shown).
    pub toggle_sidebar: Vec<String>,
    /// Toggle sidebar collapse (full/compact mode).
    pub toggle_sidebar_collapse: Vec<String>,
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
    /// Toggle the clipboard history viewer popup.
    pub toggle_clipboard_viewer: Vec<String>,
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
}


impl Default for KeybindingSettings {
    fn default() -> Self {
        Self::preset_tasty()
    }
}



mod crud;
mod presets;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
