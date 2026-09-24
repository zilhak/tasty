//! Copy/Paste 단축키.

use winit::keyboard::{Key, ModifiersState};

use crate::view::main::MainView;
use crate::view::main::selection::should_copy_via_focused_selection;
use crate::view::ui::View as _;
use tasty_key_match::matches_any_binding;

/// 단축키와 명령 팔레트가 공유하는 탐색기 파일 액션.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExplorerAction {
    SelectAll,
    CopyPath,
    CopyFiles,
    CutFiles,
    PasteFiles,
}

impl MainView {
    pub(super) fn handle_copy_shortcut(&mut self, key: &Key, mods: ModifiersState) -> bool {
        let bindings = self.core_state.settings.keybindings.copy.clone();
        if !matches_any_binding(&bindings, key, mods) {
            return false;
        }
        self.run_copy()
    }

    /// 처리하지 못하면 false로 반환해 탐색기 파일 복사 등 다음 경로로 넘긴다.
    pub(crate) fn run_copy(&mut self) -> bool {
        // 붙여넣기 직후 짧은 시간 동안은 오타 방지를 위해 복사·SIGINT 없이 안내만 한다.
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
        // 키보드 복사는 현재 포커스와 선택 영역의 surface가 같을 때만 한다.
        // 다른 surface의 이전 선택을 복사해 현재 탐색기의 입력을 가로채지 않게 한다.
        let sel_surface_id = self.text_selection.as_ref().map(|s| s.surface_id);
        let focused = self.state.focused_surface_id(&self.core_state);
        let selection_targets_focus = should_copy_via_focused_selection(sel_surface_id, focused);
        if selection_targets_focus && self.copy_selection_to_clipboard() {
            self.mark_dirty();
            return true;
        }
        let st = self.state.focused_surface_type(&self.core_state);
        // 선택 위젯은 플러그인 egui Context에 있으므로 Copy 이벤트를 해당 surface에 보낸다.
        if st.kind_capability(&self.core_state, |d| d.egui_copy)
            && let Some(sid) = self.focused_egui_mesh_surface_id()
        {
            self.egui_mesh_push_copy(sid);
            self.mark_dirty();
            return true;
        }
        false
    }

    /// copy_path capability가 있는 포커스 대상의 파일 액션을 처리한다.
    /// 파일 작업은 context menu와 같은 함수를 호출해 충돌·잘못된 붙여넣기 처리를 공유한다.
    pub(super) fn handle_explorer_shortcut(&mut self, key: &Key, mods: ModifiersState) -> bool {
        if !self
            .state
            .focused_surface_type(&self.core_state)
            .kind_capability(&self.core_state, |d| d.copy_path)
        {
            return false;
        }
        let kb = &self.core_state.settings.keybindings;
        let action = if matches_any_binding(&kb.select_all, key, mods) {
            ExplorerAction::SelectAll
        } else if matches_any_binding(&kb.copy_path, key, mods) {
            ExplorerAction::CopyPath
        } else if matches_any_binding(&kb.copy, key, mods) {
            ExplorerAction::CopyFiles
        } else if matches_any_binding(&kb.cut, key, mods) {
            ExplorerAction::CutFiles
        } else if matches_any_binding(&kb.paste, key, mods) {
            ExplorerAction::PasteFiles
        } else {
            return false;
        };
        self.run_explorer_action(action)
    }

    /// 팔레트는 키보드 앞단 검사를 거치지 않으므로 여기서도 capability를 확인한다.
    pub(crate) fn run_explorer_action(&mut self, action: ExplorerAction) -> bool {
        if !self
            .state
            .focused_surface_type(&self.core_state)
            .kind_capability(&self.core_state, |d| d.copy_path)
        {
            return false;
        }
        let is_cut_files = action == ExplorerAction::CutFiles;
        let Some(sid) = super::focused_explorer_surface_id(&self.state, &self.core_state) else {
            return true;
        };
        if action == ExplorerAction::SelectAll {
            if let Some(view) = self.state.explorer_views.get_mut(sid) {
                view.select_all();
            }
        } else if action == ExplorerAction::CopyPath {
            if let Some(text) = self
                .state
                .explorer_views
                .get(sid)
                .and_then(|v| v.selected_paths_text())
                && let Some(cb) = self.clipboard.as_mut()
            {
                cb.set_text(&text);
                self.state.toasts.push_info(
                    crate::i18n::t("toast.copied_path"),
                    crate::adapters::ui::ToastScope::Surface(sid),
                );
            }
        } else if action == ExplorerAction::CopyFiles || is_cut_files {
            let paths: Vec<std::path::PathBuf> = self
                .state
                .explorer_views
                .get(sid)
                .map(|v| v.selected.iter().cloned().collect())
                .unwrap_or_default();
            if !paths.is_empty() {
                self.explorer_menu_set_clipboard(sid, &paths, is_cut_files);
            }
        } else if action == ExplorerAction::PasteFiles
            && let Some(cwd) = super::focused_explorer_cwd(&self.state, &self.core_state)
        {
            self.explorer_menu_paste(sid, &[], &cwd, false);
        }
        self.mark_dirty();
        true
    }

    pub(super) fn handle_paste_shortcut(&mut self, key: &Key, mods: ModifiersState) -> bool {
        let bindings = self.core_state.settings.keybindings.paste.clone();
        if !matches_any_binding(&bindings, key, mods) {
            return false;
        }
        self.run_paste()
    }

    pub(crate) fn run_paste(&mut self) -> bool {
        // 키보드와 팔레트 붙여넣기 모두 사용자 입력으로 기록한다.
        if let Some(sid) = self.state.focused_surface_id(&self.core_state) {
            self.core_state.record_typing(sid);
        }
        let st = self.state.focused_surface_type(&self.core_state);
        // egui_paste는 플러그인이 처리하므로 터미널 입력으로 넘기지 않는다.
        if st.kind_capability(&self.core_state, |d| d.egui_paste) {
            return true;
        }
        self.paste_to_terminal();
        self.mark_dirty();
        true
    }
}
