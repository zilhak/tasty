use crate::model::Rect;
use crate::state::AppState;

/// Width of the sidebar in logical pixels.
const SIDEBAR_WIDTH: f32 = 180.0;
/// Height of the tab bar in logical pixels.
const TAB_BAR_HEIGHT: f32 = 32.0;

/// Render the egui UI and return the remaining terminal area rect (in physical pixels).
pub fn draw_ui(ctx: &egui::Context, state: &mut AppState, scale_factor: f32) -> Rect {
    let mut sidebar_width = SIDEBAR_WIDTH;
    let mut tab_bar_height = TAB_BAR_HEIGHT;

    // ---- Left sidebar: workspaces ----
    egui::SidePanel::left("workspace_sidebar")
        .exact_width(SIDEBAR_WIDTH)
        .resizable(false)
        .show(ctx, |ui| {
            sidebar_width = ui.available_width();

            ui.vertical(|ui| {
                ui.add_space(8.0);
                ui.heading("Workspaces");
                ui.add_space(4.0);
                ui.separator();
                ui.add_space(4.0);

                let active_ws = state.active_workspace;
                let ws_count = state.workspaces.len();

                for i in 0..ws_count {
                    let is_active = i == active_ws;
                    let name = state.workspaces[i].name.clone();

                    let label = if is_active {
                        egui::RichText::new(format!("  {} ◀", name)).strong()
                    } else {
                        egui::RichText::new(format!("  {}", name))
                    };

                    if ui
                        .selectable_label(is_active, label)
                        .clicked()
                    {
                        state.switch_workspace(i);
                    }
                }

                ui.add_space(8.0);
                if ui.button("  + New Workspace").clicked() {
                    let _ = state.add_workspace();
                }

                ui.add_space(16.0);
                ui.separator();
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new("Shortcuts")
                        .small()
                        .color(egui::Color32::GRAY),
                );
                ui.add_space(2.0);

                let shortcuts = [
                    ("Ctrl+Shift+N", "New Workspace"),
                    ("Ctrl+Shift+T", "New Pane"),
                    ("Ctrl+Tab", "Switch Pane"),
                    ("Alt+1~9", "Switch WS"),
                    ("Ctrl+Shift+E", "Split Vertical"),
                    ("Ctrl+Shift+O", "Split Horizontal"),
                    ("Alt+Arrow", "Focus Split"),
                ];

                for (key, desc) in &shortcuts {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(*key)
                                .small()
                                .color(egui::Color32::from_rgb(120, 180, 255)),
                        );
                        ui.label(egui::RichText::new(*desc).small());
                    });
                }
            });
        });

    // ---- Top tab bar: panes in active workspace ----
    egui::TopBottomPanel::top("pane_tab_bar")
        .exact_height(TAB_BAR_HEIGHT)
        .show(ctx, |ui| {
            tab_bar_height = ui.available_height();

            ui.horizontal_centered(|ui| {
                let ws = state.active_workspace_mut();
                let active_pane_idx = ws.active_pane;
                let pane_count = ws.panes.len();

                for i in 0..pane_count {
                    let is_active = i == active_pane_idx;
                    let name = ws.panes[i].name.clone();

                    let label = if is_active {
                        egui::RichText::new(&name).strong()
                    } else {
                        egui::RichText::new(&name)
                    };

                    if ui.selectable_label(is_active, label).clicked() {
                        ws.active_pane = i;
                    }
                }

                if ui.button("+").clicked() {
                    let _ = state.add_pane();
                }
            });
        });

    // Compute remaining terminal area in physical pixels
    let screen_rect = ctx.screen_rect();
    let terminal_x = SIDEBAR_WIDTH * scale_factor;
    let terminal_y = TAB_BAR_HEIGHT * scale_factor;
    let terminal_width = (screen_rect.width() - SIDEBAR_WIDTH) * scale_factor;
    let terminal_height = (screen_rect.height() - TAB_BAR_HEIGHT) * scale_factor;

    Rect {
        x: terminal_x,
        y: terminal_y,
        width: terminal_width.max(1.0),
        height: terminal_height.max(1.0),
    }
}
