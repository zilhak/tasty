//! Copy/Paste 단축키.

use winit::keyboard::{Key, ModifiersState};

use super::binding::matches_any_binding;
use crate::view::main::MainView;
use crate::view::ui::View as _;

impl MainView {
    pub(super) fn handle_copy_shortcut(&mut self, key: &Key, mods: ModifiersState) -> bool {
        let bindings = self.core_state.settings.keybindings.copy.clone();
        if !matches_any_binding(&bindings, key, mods) {
            return false;
        }
        // Paste cooldown: Ctrl+V 직후 짧은 시간 안에 들어온 Ctrl+C는 사용자의
        // 오타(옆 키 누름)로 간주하고 통째로 무시한다. SIGINT도, 클립보드 복사도
        // 일어나지 않으며 toast로만 알린다.
        if let Some(t) = self.last_terminal_paste_at
            && t.elapsed() < crate::view::main::PASTE_CTRL_C_COOLDOWN
        {
            let scope = crate::adapters::ui::ToastScope::Surface(
                self.state.focused_surface_id(&self.core_state).unwrap_or(0),
            );
            self.state
                .toasts
                .push_info(crate::i18n::t("toast.ctrl_c_ignored_after_paste"), scope);
            self.mark_dirty();
            return true;
        }
        if self.copy_selection_to_clipboard() {
            self.mark_dirty();
            return true;
        }
        let st = self.state.focused_surface_type(&self.core_state);
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

    /// Explorer 전용 선택/경로복사 단축키. clipboard 차용이 필요하므로 keybinding
    /// free-fn(=clipboard 미접근) 이 아니라 MainView 메서드로 둔다. 포커스가
    /// explorer 가 아니면 false 를 반환해 일반 단축키 경로로 흘려보낸다.
    pub(super) fn handle_explorer_shortcut(&mut self, key: &Key, mods: ModifiersState) -> bool {
        if !self
            .state
            .focused_surface_type(&self.core_state)
            .is_kind("explorer")
        {
            return false;
        }
        let kb = &self.core_state.settings.keybindings;
        let is_select_all = matches_any_binding(&kb.select_all, key, mods);
        let is_copy_path = matches_any_binding(&kb.copy_path, key, mods);
        if !is_select_all && !is_copy_path {
            return false;
        }
        let Some(sid) = super::focused_explorer_surface_id(&self.state, &self.core_state) else {
            return true;
        };
        if is_select_all {
            if let Some(view) = self.state.explorer_views.get_mut(sid) {
                view.select_all();
            }
        } else if let Some(text) = self
            .state
            .explorer_views
            .get(sid)
            .and_then(|v| v.selected_paths_text())
            && let Some(cb) = self.clipboard.as_mut()
        {
            cb.set_text(&text);
        }
        self.mark_dirty();
        true
    }

    pub(super) fn handle_paste_shortcut(&mut self, key: &Key, mods: ModifiersState) -> bool {
        let bindings = self.core_state.settings.keybindings.paste.clone();
        if !matches_any_binding(&bindings, key, mods) {
            return false;
        }
        let st = self.state.focused_surface_type(&self.core_state);
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
