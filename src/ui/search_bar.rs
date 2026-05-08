use crate::i18n::t;
use crate::state::AppState;
use crate::ui::popup::PopupAction;

/// Draw the search bar popup content.
pub fn draw_search_bar(ui: &mut egui::Ui, state: &mut AppState) -> PopupAction {
    let theme = crate::theme::theme();

    // Escape → close
    if ui.ctx().input(|i| i.key_pressed(egui::Key::Escape)) {
        state.search.clear();
        return PopupAction::Close;
    }

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;

        // Search input field
        let response = ui.add(
            egui::TextEdit::singleline(&mut state.search.query)
                .hint_text(t("search.placeholder"))
                .desired_width(200.0)
                .font(egui::TextStyle::Body),
        );

        // Auto-focus the input field
        if response.gained_focus() || ui.ctx().input(|i| i.key_pressed(egui::Key::F) && i.modifiers.command) {
            response.request_focus();
        }
        // Always keep focus on the text field while search is open
        if !response.has_focus() {
            response.request_focus();
        }

        // Run search when query changes
        if response.changed() {
            let surface_id = focused_terminal_surface_id(state);
            state.search.surface_id = surface_id;
            run_search(state);
        }

        // Enter → next match, Shift+Enter → prev match
        let enter_pressed = ui.ctx().input(|i| i.key_pressed(egui::Key::Enter));
        let shift_held = ui.ctx().input(|i| i.modifiers.shift);

        if enter_pressed {
            if shift_held {
                state.search.prev_match();
            } else {
                state.search.next_match();
            }
            scroll_to_current_match(state);
        }

        // Up/Down arrow navigation
        if ui.ctx().input(|i| i.key_pressed(egui::Key::ArrowUp)) {
            state.search.prev_match();
            scroll_to_current_match(state);
        }
        if ui.ctx().input(|i| i.key_pressed(egui::Key::ArrowDown)) {
            state.search.next_match();
            scroll_to_current_match(state);
        }

        // Match counter
        let match_text: String = if state.search.query.is_empty() {
            String::new()
        } else if state.search.matches.is_empty() {
            t("search.no_matches").to_string()
        } else {
            t("search.match_count")
                .replace("{current}", &(state.search.current_index + 1).to_string())
                .replace("{total}", &state.search.matches.len().to_string())
        };

        if !match_text.is_empty() {
            ui.label(
                egui::RichText::new(&match_text)
                    .color(if state.search.matches.is_empty() {
                        theme.red
                    } else {
                        theme.subtext0
                    })
                    .size(12.0),
            );
        }

        // Prev/Next buttons
        if !state.search.matches.is_empty() {
            if ui.small_button("▲").clicked() {
                state.search.prev_match();
                scroll_to_current_match(state);
            }
            if ui.small_button("▼").clicked() {
                state.search.next_match();
                scroll_to_current_match(state);
            }
        }

        // Case sensitivity toggle
        let case_label = if state.search.case_insensitive { "Aa" } else { "AA" };
        let case_btn = ui.small_button(
            egui::RichText::new(case_label).color(if state.search.case_insensitive {
                theme.subtext0
            } else {
                theme.text
            }),
        );
        if case_btn.clicked() {
            state.search.case_insensitive = !state.search.case_insensitive;
            run_search(state);
        }
    });

    PopupAction::None
}

/// Run search, working around borrow checker by using search fields directly.
fn run_search(state: &mut AppState) {
    let surface_id = state.search.surface_id;
    let query = state.search.query.clone();
    let case_insensitive = state.search.case_insensitive;
    if let Some(terminal) = state.find_terminal_by_id(surface_id) {
        let options = tasty_terminal::search::SearchOptions { case_insensitive };
        let matches = terminal.search(&query, &options);
        state.search.matches = matches;
        if state.search.matches.is_empty() {
            state.search.current_index = 0;
        } else if state.search.current_index >= state.search.matches.len() {
            state.search.current_index = state.search.matches.len() - 1;
        }
    }
}

fn focused_terminal_surface_id(state: &AppState) -> u32 {
    let ws = state.active_workspace();
    let pane_id = ws.focused_pane;
    ws.pane_layout()
        .find_pane(pane_id)
        .and_then(|pane| pane.tabs.get(pane.active_tab))
        .and_then(|tab| tab.focused_surface_id())
        .unwrap_or(0)
}

fn scroll_to_current_match(state: &mut AppState) {
    let surface_id = state.search.surface_id;
    if let Some(terminal) = state.find_terminal_by_id(surface_id) {
        let scrollback_len = terminal.scrollback_len();
        let screen_rows = terminal.rows();
        if let Some(offset) = state.search.scroll_to_current(scrollback_len, screen_rows) {
            if let Some(terminal) = state.find_terminal_by_id_mut(surface_id) {
                terminal.set_scroll_offset(offset);
            }
        }
    }
}
