use crate::i18n::t;
use crate::state::AppState;
use crate::ui::popup::PopupAction;
use tasty_terminal::search::{SearchError, SearchOptions};

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
                .hint_text(crate::theme_bridge::hint_text(t("search.placeholder")))
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

        // Status text: error > no_matches > match counter
        let (status_text, status_color) = if state.search.query.is_empty() {
            (String::new(), theme.subtext0)
        } else if state.search.last_error.is_some() {
            (t("search.invalid_regex").to_string(), theme.red)
        } else if state.search.matches.is_empty() {
            (t("search.no_matches").to_string(), theme.red)
        } else {
            let text = t("search.match_count")
                .replace("{current}", &(state.search.current_index + 1).to_string())
                .replace("{total}", &state.search.matches.len().to_string());
            (text, theme.subtext0)
        };

        if !status_text.is_empty() {
            ui.label(
                egui::RichText::new(&status_text)
                    .color(status_color)
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

        // Option toggles: case / regex / whole-word.
        if toggle_button(ui, "Aa", !state.search.case_insensitive, t("search.case_tooltip")) {
            state.search.case_insensitive = !state.search.case_insensitive;
            run_search(state);
        }
        if toggle_button(ui, ".*", state.search.regex, t("search.regex_tooltip")) {
            state.search.regex = !state.search.regex;
            run_search(state);
        }
        if toggle_button(ui, "ab", state.search.whole_word, t("search.whole_word_tooltip")) {
            state.search.whole_word = !state.search.whole_word;
            run_search(state);
        }
    });

    PopupAction::None
}

/// A small toggle button that visually reflects its `active` state.
/// Returns true when clicked.
fn toggle_button(ui: &mut egui::Ui, label: &str, active: bool, tooltip: impl Into<egui::WidgetText>) -> bool {
    let theme = crate::theme::theme();
    let color = if active { theme.text } else { theme.subtext0 };
    let rich = egui::RichText::new(label).color(color);
    let btn = ui.small_button(rich).on_hover_text(tooltip);
    btn.clicked()
}

/// Run search, working around borrow checker by using search fields directly.
fn run_search(state: &mut AppState) {
    let surface_id = state.search.surface_id;
    let query = state.search.query.clone();
    let options = SearchOptions {
        case_insensitive: state.search.case_insensitive,
        regex: state.search.regex,
        whole_word: state.search.whole_word,
    };
    let result = state
        .find_terminal_by_id(surface_id)
        .map(|terminal| terminal.search(&query, &options));
    match result {
        Some(Ok(matches)) => {
            state.search.matches = matches;
            state.search.last_error = None;
        }
        Some(Err(SearchError::InvalidRegex(msg))) => {
            state.search.matches.clear();
            state.search.last_error = Some(msg);
        }
        None => {}
    }
    if state.search.matches.is_empty() {
        state.search.current_index = 0;
    } else if state.search.current_index >= state.search.matches.len() {
        state.search.current_index = state.search.matches.len() - 1;
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
