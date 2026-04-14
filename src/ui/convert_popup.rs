use crate::i18n::t;
use crate::state::AppState;
use crate::theme;

/// Draw the surface type convert popup.
/// Shows Terminal / Markdown... / Explorer / Cancel options.
pub fn draw_convert_popup(ctx: &egui::Context, state: &mut AppState) {
    let surface_id = match state.convert_popup {
        Some(id) => id,
        None => return,
    };

    let th = theme::theme();

    // Determine current panel type
    let current_type = current_surface_type(state, surface_id);

    let mut close = false;
    let mut action: Option<ConvertAction> = None;

    // Check for Esc key
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        state.convert_popup = None;
        return;
    }

    let popup_w = 200.0;

    egui::Area::new(egui::Id::new("convert_popup"))
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(popup_w, 0.0), egui::Sense::hover());
            let _ = rect;

            egui::Frame::NONE
                .fill(th.surface0)
                .stroke(egui::Stroke::new(th.border_width, th.surface1))
                .corner_radius(th.corner_radius)
                .show(ui, |ui| {
                    ui.set_width(popup_w);

                    // Title
                    ui.vertical_centered(|ui| {
                        ui.add_space(6.0);
                        ui.label(egui::RichText::new(t("convert_popup.title"))
                            .color(th.text)
                            .size(th.font_size_body));
                        ui.add_space(4.0);
                    });
                    ui.separator();

                    // Menu items
                    let items: [(&str, &str, ConvertAction); 3] = [
                        ("Terminal", "convert_popup.terminal", ConvertAction::Terminal),
                        ("Markdown", "convert_popup.markdown", ConvertAction::Markdown),
                        ("Explorer", "convert_popup.explorer", ConvertAction::Explorer),
                    ];

                    for (type_name, label_key, item_action) in &items {
                        let is_current = current_type.as_deref() == Some(type_name);
                        let label = if is_current {
                            format!("✓ {}", t(label_key))
                        } else {
                            format!("  {}", t(label_key))
                        };
                        let text_color = if is_current { th.overlay0 } else { th.text };

                        let resp = ui.add_sized(
                            [popup_w, 24.0],
                            egui::Button::new(egui::RichText::new(&label).color(text_color).size(th.font_size_body))
                                .fill(egui::Color32::TRANSPARENT)
                                .frame(false),
                        );

                        if resp.clicked() && !is_current {
                            action = Some(item_action.clone());
                        }
                    }

                    ui.separator();

                    // Cancel
                    let cancel_resp = ui.add_sized(
                        [popup_w, 24.0],
                        egui::Button::new(egui::RichText::new(format!("  {}", t("convert_popup.cancel"))).color(th.subtext0).size(th.font_size_body))
                            .fill(egui::Color32::TRANSPARENT)
                            .frame(false),
                    );
                    if cancel_resp.clicked() {
                        close = true;
                    }

                    ui.add_space(4.0);
                });
        });

    // Click outside popup to close
    let primary_pressed = ctx.input(|i| i.pointer.primary_pressed());
    if primary_pressed && action.is_none() && !close {
        // Area handles its own bounds, but we check if pointer is outside
        // Since egui Area doesn't give us easy access, we close on any outside click
        // by using the memory: if nothing in the area was clicked, close
        let any_widget_hovered = ctx.is_using_pointer() || ctx.input(|i| i.pointer.has_pointer());
        if !any_widget_hovered {
            close = true;
        }
    }

    if close {
        state.convert_popup = None;
        return;
    }

    // Apply action
    if let Some(act) = action {
        state.convert_popup = None;
        match act {
            ConvertAction::Terminal => {
                state.convert_surface_to_terminal(surface_id);
            }
            ConvertAction::Markdown => {
                let pane_id = state.active_workspace().focused_pane;
                state.markdown_convert_surface_id = Some(surface_id);
                state.markdown_path_dialog = Some((pane_id, String::new()));
            }
            ConvertAction::Explorer => {
                state.convert_surface_to_explorer(surface_id);
            }
        }
    }
}

#[derive(Clone)]
enum ConvertAction {
    Terminal,
    Markdown,
    Explorer,
}

/// Get the panel type name for a specific surface ID.
fn current_surface_type(state: &AppState, surface_id: u32) -> Option<String> {
    for ws in &state.engine.workspaces {
        for &pid in &ws.pane_layout().all_pane_ids() {
            if let Some(pane) = ws.pane_layout().find_pane(pid) {
                for tab in &pane.tabs {
                    if let Some(panel) = tab.panel_if_initialized() {
                        if panel.contains_surface(surface_id) {
                            return Some(match panel {
                                crate::model::Panel::Terminal(_) => "Terminal",
                                crate::model::Panel::SurfaceGroup(_) => "SurfaceGroup",
                                crate::model::Panel::Markdown(_) => "Markdown",
                                crate::model::Panel::Explorer(_) => "Explorer",
                            }.to_string());
                        }
                    }
                }
            }
        }
    }
    None
}
