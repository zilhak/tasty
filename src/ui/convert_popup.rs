use crate::i18n::t;
use crate::state::AppState;
use crate::theme;

/// Menu items for the convert surface popup.
const ITEMS: [(&str, &str, char); 4] = [
    ("Terminal", "convert_popup.terminal", 'T'),
    ("Markdown", "convert_popup.markdown", 'M'),
    ("Explorer", "convert_popup.explorer", 'E'),
    ("Html", "convert_popup.html", 'H'),
];

/// Result of drawing the convert popup content.
pub enum ConvertResult {
    /// User selected an action.
    Action(ConvertAction),
    /// User pressed Escape or otherwise wants to close.
    Close,
}

/// Draw the convert surface popup content inside PopupManager.
/// Returns a ConvertResult if user made a choice or wants to close.
pub fn draw_convert_content(ui: &mut egui::Ui, state: &mut AppState) -> Option<ConvertResult> {
    let surface_id = state.dialogs.convert_popup?;

    let th = theme::theme();
    let current_type = current_surface_type(state, surface_id);
    let mut action: Option<ConvertAction> = None;

    let selected = state.dialogs.convert_popup_selected;
    let popup_w = ui.available_width();

    // Build selectable (non-current) indices
    let selectable_indices: Vec<usize> = ITEMS.iter().enumerate()
        .filter(|(_, (type_name, _, _))| current_type.as_deref() != Some(type_name))
        .map(|(i, _)| i)
        .collect();

    let ctx = ui.ctx().clone();

    // Escape key: close popup
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        return Some(ConvertResult::Close);
    }

    // Keyboard: Up/Down navigation
    if ctx.input(|i| i.key_pressed(egui::Key::ArrowDown)) && !selectable_indices.is_empty() {
        let new_sel = match selected {
            None => selectable_indices[0],
            Some(cur) => {
                if let Some(pos) = selectable_indices.iter().position(|&i| i == cur) {
                    selectable_indices[(pos + 1) % selectable_indices.len()]
                } else {
                    selectable_indices[0]
                }
            }
        };
        state.dialogs.convert_popup_selected = Some(new_sel);
    }

    if ctx.input(|i| i.key_pressed(egui::Key::ArrowUp)) && !selectable_indices.is_empty() {
        let new_sel = match selected {
            None => *selectable_indices.last().unwrap(),
            Some(cur) => {
                if let Some(pos) = selectable_indices.iter().position(|&i| i == cur) {
                    selectable_indices[(pos + selectable_indices.len() - 1) % selectable_indices.len()]
                } else {
                    *selectable_indices.last().unwrap()
                }
            }
        };
        state.dialogs.convert_popup_selected = Some(new_sel);
    }

    // Enter key: execute selected item
    if ctx.input(|i| i.key_pressed(egui::Key::Enter))
        && let Some(sel) = state.dialogs.convert_popup_selected
        && selectable_indices.contains(&sel)
    {
        action = Some(action_for_index(sel));
    }

    // Shortcut keys: T/M/E/H
    ctx.input(|i| {
        for event in &i.events {
            if let egui::Event::Key { key, pressed: true, modifiers, .. } = event
                && modifiers.is_none()
            {
                match key {
                    egui::Key::T if current_type.as_deref() != Some("Terminal") => {
                        action = Some(ConvertAction::Terminal);
                    }
                    egui::Key::M if current_type.as_deref() != Some("Markdown") => {
                        action = Some(ConvertAction::Markdown);
                    }
                    egui::Key::E if current_type.as_deref() != Some("Explorer") => {
                        action = Some(ConvertAction::Explorer);
                    }
                    egui::Key::H if current_type.as_deref() != Some("Html") => {
                        action = Some(ConvertAction::Html);
                    }
                    _ => {}
                }
            }
        }
    });

    // Draw menu items
    let selected = state.dialogs.convert_popup_selected;
    for (idx, (type_name, label_key, shortcut)) in ITEMS.iter().enumerate() {
        let is_current = current_type.as_deref() == Some(type_name);
        let is_selected = selected == Some(idx);

        let label = if is_current {
            format!("  \u{2713} {}    {}", t(label_key), shortcut)
        } else {
            format!("    {}    {}", t(label_key), shortcut)
        };
        let text_color = if is_current { th.overlay0 } else { th.text };

        let sense = if is_current { egui::Sense::hover() } else { egui::Sense::click() };
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(popup_w, 24.0), sense);

        // Highlight: hover or keyboard selection
        let highlight = (!is_current && resp.hovered()) || is_selected;
        if highlight {
            ui.painter().rect_filled(rect, 0.0, th.hover_overlay);
        }
        if !is_current && resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }

        let text_pos = egui::pos2(rect.min.x + th.spacing_sm, rect.center().y - th.font_size_body / 2.0);
        ui.painter().text(
            text_pos,
            egui::Align2::LEFT_TOP,
            &label,
            egui::FontId::proportional(th.font_size_body),
            text_color,
        );

        if resp.clicked() && !is_current {
            action = Some(action_for_index(idx));
        }
    }

    action.map(ConvertResult::Action)
}

/// Apply the convert action to the state.
pub fn apply_convert_action(state: &mut AppState, action: ConvertAction) {
    let Some(surface_id) = state.dialogs.convert_popup else {
        return;
    };

    match action {
        ConvertAction::Terminal => {
            state.convert_surface_to_terminal(surface_id);
        }
        ConvertAction::Markdown => {
            let pane_id = state.active_workspace().focused_pane;
            state.dialogs.markdown_convert_surface_id = Some(surface_id);
            state.dialogs.markdown_path = Some((pane_id, String::new()));
        }
        ConvertAction::Explorer => {
            state.convert_surface_to_explorer(surface_id);
        }
        ConvertAction::Html => {
            let pane_id = state.active_workspace().focused_pane;
            state.dialogs.html_convert_surface_id = Some(surface_id);
            state.dialogs.html_url = Some((pane_id, String::new()));
        }
    }
}

#[derive(Clone)]
pub enum ConvertAction {
    Terminal,
    Markdown,
    Explorer,
    Html,
}

fn action_for_index(idx: usize) -> ConvertAction {
    match idx {
        0 => ConvertAction::Terminal,
        1 => ConvertAction::Markdown,
        2 => ConvertAction::Explorer,
        _ => ConvertAction::Html,
    }
}

/// Get the panel type name for a specific surface ID.
fn current_surface_type(state: &AppState, surface_id: u32) -> Option<String> {
    for ws in &state.engine.workspaces {
        for &pid in &ws.pane_layout().all_pane_ids() {
            if let Some(pane) = ws.pane_layout().find_pane(pid)
                && let Some(tab) = pane.tabs.iter().find(|tab| {
                    tab.panel_if_initialized()
                        .is_some_and(|p| p.contains_surface(surface_id))
                })
                && let Some(panel) = tab.panel_if_initialized()
            {
                return Some(match panel {
                    crate::model::Panel::Terminal(_) => "Terminal",
                    crate::model::Panel::SurfaceGroup(_) => "SurfaceGroup",
                    crate::model::Panel::Markdown(_) => "Markdown",
                    crate::model::Panel::Explorer(_) => "Explorer",
                    crate::model::Panel::Html(_) => "Html",
                    crate::model::Panel::Empty { .. } => "Empty",
                }.to_string());
            }
        }
    }
    None
}
