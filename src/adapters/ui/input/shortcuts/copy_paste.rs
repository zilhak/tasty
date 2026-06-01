//! Copy/Paste 단축키.

use winit::keyboard::{Key, ModifiersState};

use super::binding::matches_any_binding;
use crate::adapters::ui::window::main::MainWindow;
use crate::view::ui::View as _;

impl MainWindow {
    pub(super) fn handle_copy_shortcut(&mut self, key: &Key, mods: ModifiersState) -> bool {
        let bindings = self.engine_state.settings.keybindings.copy.clone();
        if !matches_any_binding(&bindings, key, mods) {
            return false;
        }
        // Paste cooldown: Ctrl+V 직후 짧은 시간 안에 들어온 Ctrl+C는 사용자의
        // 오타(옆 키 누름)로 간주하고 통째로 무시한다. SIGINT도, 클립보드 복사도
        // 일어나지 않으며 toast로만 알린다.
        if let Some(t) = self.last_terminal_paste_at {
            if t.elapsed() < crate::adapters::ui::window::main::PASTE_CTRL_C_COOLDOWN {
                let scope = crate::adapters::ui::ToastScope::Surface(
                    self.state
                        .focused_surface_id(&self.engine_state)
                        .unwrap_or(0),
                );
                self.state
                    .toasts
                    .push_info(crate::i18n::t("toast.ctrl_c_ignored_after_paste"), scope);
                self.mark_dirty();
                return true;
            }
        }
        if self.copy_selection_to_clipboard() {
            self.mark_dirty();
            return true;
        }
        let st = self.state.focused_surface_type(&self.engine_state);
        if st.is_kind("markdown") {
            self.base
                .gpu
                .egui_ctx
                .input_mut(|i| i.events.push(egui::Event::Copy));
            self.mark_dirty();
            return true;
        }
        false
    }

    pub(super) fn handle_paste_shortcut(&mut self, key: &Key, mods: ModifiersState) -> bool {
        let bindings = self.engine_state.settings.keybindings.paste.clone();
        if !matches_any_binding(&bindings, key, mods) {
            return false;
        }
        let st = self.state.focused_surface_type(&self.engine_state);
        if st.is_kind("image") {
            if self.paste_to_image() {
                self.mark_dirty();
            }
            return true;
        }
        self.paste_to_terminal();
        self.mark_dirty();
        true
    }
}
