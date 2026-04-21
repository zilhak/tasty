use crate::i18n::t;
use crate::state::AppState;
use crate::theme;
use crate::ui::popup::{self, PopupAction};

/// Item height in the convert popup menu.
const ITEM_HEIGHT: f32 = 24.0;

/// Default size for the convert surface popup.
pub fn convert_popup_default_size() -> egui::Vec2 {
    let item_spacing = 3.0;
    let content_h =
        ITEMS.len() as f32 * ITEM_HEIGHT + (ITEMS.len().saturating_sub(1)) as f32 * item_spacing;
    egui::vec2(
        200.0,
        popup::TITLE_BAR_HEIGHT + popup::CONTENT_MARGIN * 2.0 + content_h,
    )
}

/// PopupDef::draw_fn entry point for the convert surface popup.
pub fn draw_convert_popup(ui: &mut egui::Ui, state: &mut AppState) -> PopupAction {
    match draw_convert_content(ui, state) {
        Some(ConvertResult::Close) => PopupAction::Close,
        Some(ConvertResult::Action(action)) => {
            apply_convert_action(state, action);
            PopupAction::Close
        }
        None => PopupAction::None,
    }
}

/// Menu items for the convert surface popup.
const ITEMS: [(&str, &str, char); 5] = [
    ("Terminal", "convert_popup.terminal", 'T'),
    ("Markdown", "convert_popup.markdown", 'M'),
    ("Explorer", "convert_popup.explorer", 'E'),
    ("Html", "convert_popup.html", 'H'),
    ("Image", "convert_popup.image", 'I'),
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
    let selectable_indices: Vec<usize> = ITEMS
        .iter()
        .enumerate()
        .filter(|(_, (type_name, _, _))| current_type != Some(type_name))
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
                    selectable_indices
                        [(pos + selectable_indices.len() - 1) % selectable_indices.len()]
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

    // Shortcut keys: T/M/E/H/I
    // physical_key를 사용하여 한글 IME 활성 시에도 올바르게 매칭한다.
    // 팝업이 열리면 gpu/mod.rs에서 set_ime_allowed(false)를 호출하여
    // OS가 KeyboardInput을 직접 발생시키므로 physical_key가 항상 유효하다.
    ctx.input(|i| {
        for event in &i.events {
            if let egui::Event::Key {
                physical_key,
                pressed: true,
                modifiers,
                ..
            } = event
                && modifiers.is_none()
            {
                let matched_key = physical_key.as_ref().unwrap_or(&egui::Key::Escape);
                match matched_key {
                    egui::Key::T if current_type != Some("Terminal") => {
                        action = Some(ConvertAction::Terminal);
                    }
                    egui::Key::M if current_type != Some("Markdown") => {
                        action = Some(ConvertAction::Markdown);
                    }
                    egui::Key::E if current_type != Some("Explorer") => {
                        action = Some(ConvertAction::Explorer);
                    }
                    egui::Key::H if current_type != Some("Html") => {
                        action = Some(ConvertAction::Html);
                    }
                    egui::Key::I if current_type != Some("Image") => {
                        action = Some(ConvertAction::Image);
                    }
                    _ => {}
                }
            }
        }
    });

    // Draw menu items
    let selected = state.dialogs.convert_popup_selected;
    for (idx, (type_name, label_key, shortcut)) in ITEMS.iter().enumerate() {
        let is_current = current_type == Some(type_name);
        let is_selected = selected == Some(idx);

        let label = if is_current {
            format!("  \u{2713} {}    {}", t(label_key), shortcut)
        } else {
            format!("    {}    {}", t(label_key), shortcut)
        };
        let text_color = if is_current { th.overlay0 } else { th.text };

        let sense = if is_current {
            egui::Sense::hover()
        } else {
            egui::Sense::click()
        };
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(popup_w, 24.0), sense);

        // Highlight: hover or keyboard selection
        let highlight = (!is_current && resp.hovered()) || is_selected;
        if highlight {
            ui.painter().rect_filled(rect, 0.0, th.hover_overlay);
        }
        if !is_current && resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }

        let text_pos = egui::pos2(
            rect.min.x + th.spacing_sm.value(),
            rect.center().y - th.font_size_body.value() / 2.0,
        );
        ui.painter().text(
            text_pos,
            egui::Align2::LEFT_TOP,
            &label,
            egui::FontId::proportional(th.font_size_body.value()),
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
            state.dialogs.file_open_pane_id = Some(pane_id);
            state.dialogs.markdown_open_buffer.clear();
            state.dialogs.pending_popup_open =
                Some(("markdown_open", popup::PopupScope::Surface(surface_id)));
        }
        ConvertAction::Explorer => {
            state.convert_surface_to_explorer(surface_id);
        }
        ConvertAction::Html => {
            let pane_id = state.active_workspace().focused_pane;
            state.dialogs.html_convert_surface_id = Some(surface_id);
            state.dialogs.file_open_pane_id = Some(pane_id);
            state.dialogs.html_open_buffer.clear();
            state.dialogs.pending_popup_open =
                Some(("html_open", popup::PopupScope::Surface(surface_id)));
        }
        ConvertAction::Image => {
            state.convert_surface_to_image(surface_id);
        }
    }
}

#[derive(Clone)]
pub enum ConvertAction {
    Terminal,
    Markdown,
    Explorer,
    Html,
    Image,
}

fn action_for_index(idx: usize) -> ConvertAction {
    match idx {
        0 => ConvertAction::Terminal,
        1 => ConvertAction::Markdown,
        2 => ConvertAction::Explorer,
        3 => ConvertAction::Html,
        _ => ConvertAction::Image,
    }
}

/// Get the type name for a specific surface ID.
/// If the surface is inside a split tab, returns the individual leaf's type.
fn current_surface_type(state: &AppState, surface_id: u32) -> Option<&'static str> {
    for ws in &state.engine.workspaces {
        for &pid in &ws.pane_layout().all_pane_ids() {
            if let Some(pane) = ws.pane_layout().find_pane(pid) {
                for tab in &pane.tabs {
                    if !tab.contains_surface(surface_id) {
                        continue;
                    }
                    // Find the specific leaf in the layout.
                    if let Some(leaf) = tab.layout().find_surface(surface_id) {
                        return Some(leaf.type_name());
                    }
                    // Fallback: return the focused surface's type.
                    return Some(tab.surface().type_name());
                }
            }
        }
    }
    None
}
