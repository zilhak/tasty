use crate::i18n::t;
use crate::state::{AppState, WsRenameField};

/// Draw the workspace rename dialog (if active).
pub fn draw_ws_rename_dialog(ctx: &egui::Context, state: &mut AppState) {
    let Some((ws_idx, field, ref mut buffer)) = state.dialogs.ws_rename else {
        return;
    };

    if ws_idx >= state.engine.workspaces.len() {
        state.dialogs.ws_rename = None;
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
        let (ws_idx, field, buffer) = state.dialogs.ws_rename.take().unwrap();
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
        state.dialogs.ws_rename = None;
    }
}
