use crate::i18n::t;
use crate::state::{AppState, WsRenameField};

/// Draw the workspace rename dialog (if active).
pub fn draw_ws_rename_dialog(ctx: &egui::Context, state: &mut AppState) {
    let Some((ws_idx, field, ref mut buffer)) = state.ws_rename else {
        return;
    };

    if ws_idx >= state.engine.workspaces.len() {
        state.ws_rename = None;
        return;
    }

    let heading = match field {
        WsRenameField::Name => t("rename_dialog.title_heading"),
        WsRenameField::Subtitle => t("rename_dialog.subtitle_heading"),
    };

    let mut do_apply = false;
    let mut do_cancel = false;

    egui::Window::new(heading)
        .fixed_size(egui::vec2(280.0, 60.0))
        .collapsible(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ctx, |ui| {
            let response = ui.text_edit_singleline(buffer);
            // Auto-focus the text field on first frame
            if !response.has_focus() {
                response.request_focus();
            }
            // Enter to confirm, Escape to cancel
            if response.lost_focus() {
                if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    do_apply = true;
                } else if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    do_cancel = true;
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
        let (ws_idx, field, buffer) = state.ws_rename.take().unwrap();
        if ws_idx < state.engine.workspaces.len() {
            match field {
                WsRenameField::Name => {
                    if !buffer.is_empty() {
                        state.engine.workspaces[ws_idx].name = buffer;
                    }
                }
                WsRenameField::Subtitle => {
                    state.engine.workspaces[ws_idx].subtitle = buffer;
                }
            }
        }
    } else if do_cancel {
        state.ws_rename = None;
    }
}

/// Render the markdown file path dialog.
pub fn draw_markdown_path_dialog(
    ctx: &egui::Context,
    state: &mut AppState,
) {
    let (pane_id, mut path_buf) = match state.markdown_path_dialog.take() {
        Some(d) => d,
        None => return,
    };

    let mut keep_open = true;
    let mut confirm = false;

    egui::Window::new("Open Markdown File")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.label("Enter file path:");
            let response = ui.text_edit_singleline(&mut path_buf);
            if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                confirm = true;
            }
            // Request focus on first frame
            response.request_focus();
            ui.horizontal(|ui| {
                if ui.button("OK").clicked() {
                    confirm = true;
                }
                if ui.button("Cancel").clicked() {
                    keep_open = false;
                }
            });
        });

    if confirm && !path_buf.is_empty() {
        state.active_workspace_mut().focused_pane = pane_id;
        let _ = state.add_markdown_tab(path_buf);
        // Don't keep dialog open
    } else if keep_open && !confirm {
        state.markdown_path_dialog = Some((pane_id, path_buf));
    }
}
