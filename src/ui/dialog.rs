use crate::i18n::t;
use crate::state::{AppState, RenameTarget};

/// Draw the unified rename dialog (workspace name/subtitle, tab name).
pub fn draw_rename_dialog(ctx: &egui::Context, state: &mut AppState) {
    let Some((target, _)) = &state.dialogs.rename else {
        return;
    };

    // Validate target still exists (read-only check before mutable borrow).
    let valid = match target {
        RenameTarget::WorkspaceName { ws_idx } | RenameTarget::WorkspaceSubtitle { ws_idx } => {
            *ws_idx < state.engine.workspaces.len()
        }
        RenameTarget::TabName {
            pane_id,
            tab_index,
        } => state
            .active_workspace()
            .pane_layout()
            .find_pane(*pane_id)
            .is_some_and(|p| *tab_index < p.tabs.len()),
    };
    if !valid {
        state.dialogs.rename = None;
        return;
    }

    let heading = t(target.heading_key());

    let mut do_apply = false;
    let mut do_cancel = false;

    // Re-borrow mutably for the text buffer.
    let buffer = &mut state.dialogs.rename.as_mut().unwrap().1;

    egui::Window::new(heading)
        .fixed_size(egui::vec2(280.0, 60.0))
        .collapsible(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ctx, |ui| {
            let response = ui.text_edit_singleline(buffer);

            // ── Focus handling (egui 0.31 동적 focus 계산 대응) ──
            // lost_focus() = `had_focus_last_frame && !has_focus` (동적 계산)
            // → request_focus() 이전에 먼저 체크해야 한다.
            let focus_lost = response.lost_focus();

            // Auto-focus: 포커스가 없으면 되찾는다.
            if !response.has_focus() {
                response.request_focus();
            }

            // gained_focus() = `!had_focus_last_frame && has_focus` (동적 계산)
            // → request_focus() 이후에 체크해야 첫 프레임에서 true가 된다.
            if response.gained_focus() {
                if let Some(mut text_state) = egui::TextEdit::load_state(ctx, response.id) {
                    let len = buffer.chars().count();
                    text_state.cursor.set_char_range(Some(egui::text::CCursorRange::two(
                        egui::text::CCursor::new(0),
                        egui::text::CCursor::new(len),
                    )));
                    text_state.store(ctx, response.id);
                }
            }

            if focus_lost {
                if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    do_cancel = true;
                } else {
                    do_apply = true;
                }
            }

            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(t("button.cancel")).clicked() {
                        do_cancel = true;
                    }
                    if ui.button(t("button.save")).clicked() {
                        do_apply = true;
                    }
                });
            });
        });

    if do_apply {
        let (target, buffer) = state.dialogs.rename.take().unwrap();
        apply_rename(state, target, buffer);
    } else if do_cancel {
        state.dialogs.rename = None;
    }
}

fn apply_rename(state: &mut AppState, target: RenameTarget, buffer: String) {
    match target {
        RenameTarget::WorkspaceName { ws_idx } => {
            if !buffer.is_empty() {
                if let Some(ws) = state.engine.workspaces.get_mut(ws_idx) {
                    ws.name = buffer;
                }
            }
        }
        RenameTarget::WorkspaceSubtitle { ws_idx } => {
            if let Some(ws) = state.engine.workspaces.get_mut(ws_idx) {
                ws.subtitle = buffer;
            }
        }
        RenameTarget::TabName {
            pane_id,
            tab_index,
        } => {
            let name = buffer.trim().to_string();
            if let Some(pane) = state
                .active_workspace_mut()
                .pane_layout_mut()
                .find_pane_mut(pane_id)
            {
                if let Some(tab) = pane.tabs.get_mut(tab_index) {
                    if name.is_empty() {
                        tab.explicit_name = None;
                    } else {
                        tab.explicit_name = Some(name);
                    }
                }
            }
        }
    }
    state.engine.mark_layout_dirty();
}
