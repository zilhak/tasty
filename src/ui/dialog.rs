use crate::i18n::t;
use crate::state::{AppState, RenameTarget};
use crate::theme;
use crate::ui::popup::{CONTENT_MARGIN, PopupAction, TITLE_BAR_HEIGHT};

/// Default size for the rename popup.
pub fn rename_popup_default_size() -> egui::Vec2 {
    egui::vec2(280.0, TITLE_BAR_HEIGHT + CONTENT_MARGIN * 2.0 + 64.0)
}

/// Dynamic title for the rename popup (based on RenameTarget).
pub fn rename_popup_title(state: &AppState) -> String {
    state
        .dialogs
        .rename
        .as_ref()
        .map(|(target, _)| t(target.heading_key()).to_string())
        .unwrap_or_else(|| t("rename_dialog.tab_heading").to_string())
}

/// Draw function for the rename popup (PopupDef draw_fn).
pub fn draw_rename_popup(ui: &mut egui::Ui, state: &mut AppState) -> PopupAction {
    let th = theme::theme();
    let ctx = ui.ctx().clone();

    // Escape → cancel
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        state.dialogs.rename = None;
        return PopupAction::Close;
    }

    let Some((ref target, _)) = state.dialogs.rename else {
        return PopupAction::Close;
    };

    // Validate target still exists.
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
        return PopupAction::Close;
    }

    let margin = 8.0;
    let available = ui.available_rect_before_wrap();
    let inner_rect = available.shrink2(egui::vec2(margin, 2.0));
    let mut child_ui = ui.new_child(egui::UiBuilder::new().max_rect(inner_rect));
    let ui = &mut child_ui;

    let buffer = &mut state.dialogs.rename.as_mut().unwrap().1;

    let resp = ui.add_sized(
        [ui.available_width(), 22.0],
        egui::TextEdit::singleline(buffer)
            .font(egui::FontId::proportional(th.font_size_body.value()))
            .margin(egui::Margin::symmetric(4, 2)),
    );

    // Auto-focus: 포커스가 없으면 되찾는다.
    if !resp.has_focus() {
        resp.request_focus();
    }

    // 포커스를 얻은 첫 프레임에 전체 선택
    if resp.gained_focus() {
        if let Some(mut text_state) = egui::TextEdit::load_state(&ctx, resp.id) {
            let len = buffer.chars().count();
            text_state
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::two(
                    egui::text::CCursor::new(0),
                    egui::text::CCursor::new(len),
                )));
            text_state.store(&ctx, resp.id);
        }
    }

    // Enter 키로 적용
    let mut confirm = false;
    if resp.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
        confirm = true;
    }

    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button(t("button.cancel")).clicked() {
                state.dialogs.rename = None;
                state.dialogs.file_popup_cancel = true;
            }
            if ui.button(t("button.save")).clicked() {
                confirm = true;
            }
        });
    });

    if state.dialogs.file_popup_cancel {
        state.dialogs.file_popup_cancel = false;
        return PopupAction::Close;
    }

    if confirm {
        let (target, buffer) = state.dialogs.rename.take().unwrap();
        apply_rename(state, target, buffer);
        return PopupAction::Close;
    }

    PopupAction::None
}

fn apply_rename(state: &mut AppState, target: RenameTarget, buffer: String) {
    match target {
        RenameTarget::WorkspaceName { ws_idx } => {
            if !buffer.is_empty() {
                let workspace_id = state.engine.workspaces.get(ws_idx).map(|w| w.id);
                if let Some(ws) = state.engine.workspaces.get_mut(ws_idx) {
                    ws.name = buffer.clone();
                }
                if let Some(workspace_id) = workspace_id {
                    state.enqueue_host_event(crate::state::PendingHostEvent::WorkspaceRenamed {
                        workspace_id,
                        name: Some(buffer),
                        subtitle: None,
                        description: None,
                    });
                }
            }
        }
        RenameTarget::WorkspaceSubtitle { ws_idx } => {
            let workspace_id = state.engine.workspaces.get(ws_idx).map(|w| w.id);
            if let Some(ws) = state.engine.workspaces.get_mut(ws_idx) {
                ws.subtitle = buffer.clone();
            }
            if let Some(workspace_id) = workspace_id {
                state.enqueue_host_event(crate::state::PendingHostEvent::WorkspaceRenamed {
                    workspace_id,
                    name: None,
                    subtitle: Some(buffer),
                    description: None,
                });
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
