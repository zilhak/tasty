use crate::model::Rect;
use crate::state::AppState;

/// Width of the sidebar in logical pixels.
const SIDEBAR_WIDTH: f32 = 180.0;

/// Render the egui UI and return the remaining terminal area rect (in physical pixels).
pub fn draw_ui(ctx: &egui::Context, state: &mut AppState, scale_factor: f32) -> Rect {
    // ---- Left sidebar: workspaces ----
    egui::SidePanel::left("workspace_sidebar")
        .exact_width(SIDEBAR_WIDTH)
        .resizable(false)
        .show(ctx, |ui| {
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

                    if ui.selectable_label(is_active, label).clicked() {
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
                    ("Ctrl+Shift+T", "New Tab"),
                    ("Ctrl+Tab", "Next Tab"),
                    ("Ctrl+Shift+Tab", "Prev Tab"),
                    ("Alt+1~9", "Switch WS"),
                    ("Ctrl+Shift+E", "Pane Split V"),
                    ("Ctrl+Shift+O", "Pane Split H"),
                    ("Ctrl+D", "Surface Split V"),
                    ("Ctrl+Shift+D", "Surface Split H"),
                    ("Alt+Arrow", "Focus Pane"),
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

    // No global top panel - tab bars are now per-pane and rendered in gpu.rs

    // Compute remaining terminal area in physical pixels
    let screen_rect = ctx.screen_rect();
    let terminal_x = SIDEBAR_WIDTH * scale_factor;
    let terminal_y = 0.0; // No global tab bar anymore
    let terminal_width = (screen_rect.width() - SIDEBAR_WIDTH) * scale_factor;
    let terminal_height = screen_rect.height() * scale_factor;

    Rect {
        x: terminal_x,
        y: terminal_y,
        width: terminal_width.max(1.0),
        height: terminal_height.max(1.0),
    }
}

/// Draw per-pane tab bars using egui Areas positioned at each pane's top.
/// This is called during the egui frame (from gpu.rs render).
pub fn draw_pane_tab_bars(
    ctx: &egui::Context,
    state: &mut AppState,
    pane_rects: &[(u32, Rect)],
    scale_factor: f32,
) {
    let focused_pane_id = state.focused_pane_id();

    // First pass: gather tab info (read-only) and render UI, collecting user actions.
    struct PaneTabInfo {
        pane_id: u32,
        tab_names: Vec<String>,
        active_tab: usize,
        is_focused: bool,
        logical_x: f32,
        logical_y: f32,
        logical_w: f32,
    }

    let mut infos = Vec::new();
    {
        let ws = state.active_workspace();
        for &(pane_id, pane_rect) in pane_rects {
            let pane = match ws.pane_layout.find_pane(pane_id) {
                Some(p) => p,
                None => continue,
            };
            if pane.tabs.len() <= 1 {
                continue;
            }
            infos.push(PaneTabInfo {
                pane_id,
                tab_names: pane.tabs.iter().map(|t| t.name.clone()).collect(),
                active_tab: pane.active_tab,
                is_focused: pane_id == focused_pane_id,
                logical_x: pane_rect.x / scale_factor,
                logical_y: pane_rect.y / scale_factor,
                logical_w: pane_rect.width / scale_factor,
            });
        }
    }

    // Second pass: render egui and collect actions.
    let mut actions: Vec<(u32, PaneTabAction)> = Vec::new();

    for info in &infos {
        egui::Area::new(egui::Id::new(format!("pane_tabs_{}", info.pane_id)))
            .fixed_pos(egui::pos2(info.logical_x, info.logical_y))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                ui.set_min_width(info.logical_w);
                ui.set_max_width(info.logical_w);

                let bg = if info.is_focused {
                    egui::Color32::from_rgb(40, 40, 48)
                } else {
                    egui::Color32::from_rgb(30, 30, 36)
                };

                egui::Frame::new()
                    .fill(bg)
                    .inner_margin(egui::Margin::symmetric(4, 2))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            for (i, name) in info.tab_names.iter().enumerate() {
                                let is_active = i == info.active_tab;
                                let label = if is_active {
                                    egui::RichText::new(name).strong().small()
                                } else {
                                    egui::RichText::new(name).small()
                                };

                                if ui.selectable_label(is_active, label).clicked() {
                                    actions.push((info.pane_id, PaneTabAction::SwitchTab(i)));
                                }
                            }

                            if ui.small_button("+").clicked() {
                                actions.push((info.pane_id, PaneTabAction::AddTab));
                            }
                        });
                    });
            });
    }

    // Third pass: apply actions.
    for (pane_id, action) in actions {
        match action {
            PaneTabAction::SwitchTab(idx) => {
                if let Some(pane) = state.active_workspace_mut().pane_layout.find_pane_mut(pane_id) {
                    pane.active_tab = idx;
                }
            }
            PaneTabAction::AddTab => {
                // Focus this pane first, then add tab
                state.active_workspace_mut().focused_pane = pane_id;
                let _ = state.add_tab();
            }
        }
    }
}

enum PaneTabAction {
    SwitchTab(usize),
    AddTab,
}
