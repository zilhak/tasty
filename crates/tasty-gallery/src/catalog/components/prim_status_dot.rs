//! StatusDot primitive specimen — 디자인 gallery `components.html` 대조.

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{status_dot, StatusKind};

use crate::catalog::specimen::caption;

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    ui.spacing_mut().item_spacing = egui::vec2(16.0, 10.0);

    caption(ui, theme, "StatusDot — running(pulse) · idle · agent · waiting · error");
    ui.horizontal(|ui| {
        status_dot(ui, theme, StatusKind::Running, "LISTEN", true, false);
        status_dot(ui, theme, StatusKind::Idle, "idle", false, false);
        status_dot(ui, theme, StatusKind::Agent, "agent", true, false);
        status_dot(ui, theme, StatusKind::Waiting, "CLOSE_WAIT", false, false);
        status_dot(ui, theme, StatusKind::Error, "error", false, false);
    });

    ui.add_space(10.0);
    caption(ui, theme, "reduced motion — pulse 생략");
    ui.horizontal(|ui| {
        status_dot(ui, theme, StatusKind::Running, "LISTEN", true, true);
        status_dot(ui, theme, StatusKind::Agent, "agent", true, true);
    });
}
