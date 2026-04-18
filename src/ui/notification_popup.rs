//! Notification panel popup draw entry point.

use crate::state::AppState;
use crate::ui::popup::PopupAction;

/// PopupDef::draw_fn for the notifications panel.
pub fn draw_notification_popup(ui: &mut egui::Ui, state: &mut AppState) -> PopupAction {
    super::notification::draw_notification_content_inner(ui, state);
    PopupAction::None
}
