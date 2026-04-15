use crate::state::AppState;
use crate::ui::popup::{PopupAction, PopupContent, PopupId, PopupScope};

/// Notification panel popup (Window scope, always visible).
pub struct NotificationPopup;

impl NotificationPopup {
    pub fn new() -> Self {
        Self
    }
}

impl PopupContent for NotificationPopup {
    fn id(&self) -> PopupId {
        "notifications"
    }

    fn title(&self) -> String {
        "Notifications".to_string()
    }

    fn default_size(&self) -> egui::Vec2 {
        egui::vec2(350.0, 400.0)
    }

    fn scope(&self) -> PopupScope {
        PopupScope::Window
    }

    fn draw(&mut self, ui: &mut egui::Ui, state: &mut AppState) -> PopupAction {
        super::notification::draw_notification_content_inner(ui, state);
        PopupAction::None
    }
}
