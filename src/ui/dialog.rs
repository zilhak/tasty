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
            // 다이얼로그가 막 열려 포커스를 얻은 첫 프레임에 텍스트 전체를 선택해서,
            // 사용자가 곧바로 입력하면 기존 이름이 새 입력으로 대체되도록 한다.
            if response.gained_focus() {
                if let Some(mut text_state) =
                    egui::TextEdit::load_state(ctx, response.id)
                {
                    let len = buffer.chars().count();
                    text_state.cursor.set_char_range(Some(
                        egui::text::CCursorRange::two(
                            egui::text::CCursor::new(0),
                            egui::text::CCursor::new(len),
                        ),
                    ));
                    text_state.store(ctx, response.id);
                }
            }
            // Enter to confirm, Escape to cancel.
            // Check key_pressed first: singleline TextEdit surrenders focus on Enter,
            // but key_pressed may already be consumed by the widget in the same frame.
            // Use lost_focus() as the primary trigger — it fires reliably on Enter.
            if response.lost_focus() {
                // lost_focus on singleline TextEdit happens on Enter or Escape.
                // Escape can be distinguished because the user pressed Escape key.
                if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    do_cancel = true;
                } else {
                    // lost_focus without Escape = Enter (or clicked outside)
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
            state.engine.mark_layout_dirty();
        }
    } else if do_cancel {
        state.dialogs.ws_rename = None;
    }
}
