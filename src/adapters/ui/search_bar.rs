use crate::adapters::ui::popup::PopupAction;
use crate::i18n::t;
use crate::state::AppState;
use tasty_terminal::search::{SearchError, SearchOptions};

/// Draw the search bar popup content.
pub fn draw_search_bar(
    ui: &mut egui::Ui,
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
) -> PopupAction {
    let theme = crate::theme::theme();

    // 키보드 포커스가 검색창에 있어야 하는지 — 포커스 토글의 단일 진실원
    // (`popup.focused`). egui 텍스트필드 포커스는 이 값을 따라간다.
    let want_focus = state.popups.is_focused("search_bar");

    let mut action = PopupAction::None;

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;

        // Search input field
        let response = ui.add(
            egui::TextEdit::singleline(&mut state.search.query)
                .hint_text(tasty_egui_theme::hint_text(
                    &crate::theme::theme(),
                    t("search.placeholder"),
                ))
                .desired_width(200.0)
                .font(egui::TextStyle::Body),
        );

        // popup.focused 에 맞춰 egui 텍스트필드 포커스를 동기화한다. 열릴 때/검색창으로
        // 토글될 때만 1회 요청 (이미 포커스면 재요청하지 않음 — 매 프레임 강제 포커스 금지).
        if want_focus && !response.has_focus() {
            response.request_focus();
        }

        // 검색 필드가 실제로 포커스일 때만 키 입력을 해석한다. 터미널 포커스 상태
        // (검색창은 떠 있으나 비포커스)에서는 어떤 키도 가로채지 않고 PTY 로 흘려보낸다.
        if response.has_focus() {
            // find 단축키 → 검색창은 그대로 두고 포커스만 터미널로 되돌린다.
            let find_pressed = ui.input(|i| {
                crate::adapters::ui::input::shortcuts::any_binding_pressed_egui(
                    &engine.settings.keybindings.find,
                    i,
                )
            });
            if find_pressed {
                response.surrender_focus();
                state.popups.set_focused("search_bar", false);
            } else if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                // Escape → 닫기 + 검색 상태 clear (하이라이트 제거) + 터미널 포커스 복귀.
                state.search.clear();
                action = PopupAction::Close;
            } else {
                // Run search when query changes
                if response.changed() {
                    let surface_id = focused_terminal_surface_id(state, engine);
                    state.search.surface_id = surface_id;
                    run_search(state, engine);
                }

                // Enter → next match, Shift+Enter → prev match
                let enter_pressed = ui.input(|i| i.key_pressed(egui::Key::Enter));
                let shift_held = ui.input(|i| i.modifiers.shift);

                if enter_pressed {
                    if shift_held {
                        state.search.prev_match();
                    } else {
                        state.search.next_match();
                    }
                    scroll_to_current_match(state, engine);
                }

                // Up/Down arrow navigation
                if ui.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
                    state.search.prev_match();
                    scroll_to_current_match(state, engine);
                }
                if ui.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
                    state.search.next_match();
                    scroll_to_current_match(state, engine);
                }
            }
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
                scroll_to_current_match(state, engine);
            }
            if ui.small_button("▼").clicked() {
                state.search.next_match();
                scroll_to_current_match(state, engine);
            }
        }

        // Option toggles: case / regex / whole-word.
        if toggle_button(
            ui,
            "Aa",
            !state.search.case_insensitive,
            t("search.case_tooltip"),
        ) {
            state.search.case_insensitive = !state.search.case_insensitive;
            run_search(state, engine);
        }
        if toggle_button(ui, ".*", state.search.regex, t("search.regex_tooltip")) {
            state.search.regex = !state.search.regex;
            run_search(state, engine);
        }
        if toggle_button(
            ui,
            "ab",
            state.search.whole_word,
            t("search.whole_word_tooltip"),
        ) {
            state.search.whole_word = !state.search.whole_word;
            run_search(state, engine);
        }
    });

    action
}

/// A small toggle button that visually reflects its `active` state.
/// Returns true when clicked.
fn toggle_button(
    ui: &mut egui::Ui,
    label: &str,
    active: bool,
    tooltip: impl Into<egui::WidgetText>,
) -> bool {
    let theme = crate::theme::theme();
    let color = if active { theme.text } else { theme.subtext0 };
    let rich = egui::RichText::new(label).color(color);
    let btn = ui.small_button(rich).on_hover_text(tooltip);
    btn.clicked()
}

/// Run search, working around borrow checker by using search fields directly.
fn run_search(state: &mut AppState, engine: &crate::core::CoreState) {
    let surface_id = state.search.surface_id;
    let query = state.search.query.clone();
    let options = SearchOptions {
        case_insensitive: state.search.case_insensitive,
        regex: state.search.regex,
        whole_word: state.search.whole_word,
    };
    let result = engine
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

fn focused_terminal_surface_id(state: &AppState, engine: &crate::core::CoreState) -> u32 {
    let ws = state.active_workspace(engine);
    let pane_id = ws.focused_pane;
    ws.pane_layout()
        .find_pane(pane_id)
        .and_then(|pane| pane.tabs.get(pane.active_tab))
        .and_then(|tab| tab.focused_surface_id())
        .unwrap_or(0)
}

fn scroll_to_current_match(state: &mut AppState, engine: &mut crate::core::CoreState) {
    let surface_id = state.search.surface_id;
    if let Some(terminal) = engine.find_terminal_by_id(surface_id) {
        let scrollback_len = terminal.scrollback_len();
        let screen_rows = terminal.rows();
        if let Some(offset) = state.search.scroll_to_current(scrollback_len, screen_rows)
            && let Some(terminal) = engine.find_terminal_by_id_mut(surface_id)
        {
            terminal.set_scroll_offset(offset);
        }
    }
}
