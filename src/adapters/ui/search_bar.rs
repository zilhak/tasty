use crate::adapters::ui::icons;
use crate::adapters::ui::popup::PopupAction;
use crate::i18n::t;
use crate::state::AppState;
use crate::theme::Theme;
use tasty_terminal::search::{SearchError, SearchOptions};
use tasty_type_geometry::length::LogicalPx;

// ── 디자인 스케일 밖 폰트 크기 ──────────────────────────────────────────────
//
// **`.5` 로 끝나는 값은 애초에 토큰이 될 수 없다** — 토큰 폰트 크기는 `zoomed()` 의
// `.round()` 를 거쳐 어떤 `ui_scale` 에서도 정수다. semantic 이 없는 primitive(12)도
// 같은 이유로 이름만 붙인다. 규칙 전문은 `docs/design/systems/theme.md`
// "스케일 밖 폰트 값".

/// 매치 카운터(`3/17`) 폰트. DTCG primitive `font-size-12` 는 있으나 semantic role 이
/// 없어 `Theme` 필드가 없다 — ADR-0126 대로 **이름에 primitive 임을 남긴다**.
const COUNTER_FONT_PRIMITIVE_12: LogicalPx = LogicalPx(12.0);

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
        ui.spacing_mut().item_spacing.x = theme.spacing_xs.value();

        // Search input field — flex: 남은 가용 폭을 채우되 최소 60px. (디자인 canonical:
        // search_bar.jsx `flex:1; minWidth:60`.) 뒤따르는 고정폭 요소(카운터 40px +
        // IconButton sm 5개)와 그 사이 간격을 가용 폭에서 빼서 input 폭을 산출한다.
        let spacing = ui.spacing().item_spacing.x;
        let btn_size = theme.item_height_tab.value();
        let divider_width = theme.border_width.value();
        // 카운터(40) + nav 2개 + 토글 3개 + close 1개 = 6개 버튼, input 뒤로 8개의
        // 간격 + divider 폭 1개.
        let reserved = 40.0 + 6.0 * btn_size + 8.0 * spacing + divider_width;
        let input_width = (ui.available_width() - reserved).max(60.0);

        let response = ui.add(
            egui::TextEdit::singleline(&mut state.search.query)
                .hint_text(tasty_egui_theme::hint_text(
                    &crate::theme::theme(),
                    t("search.placeholder"),
                ))
                .desired_width(input_width)
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

        // Status counter — 항상 고정폭(40px)으로 렌더. 빈 쿼리 / 무매치 / 정규식
        // 에러 모두 `0/0`. 쿼리가 있는데 결과가 0(에러 포함)이면 danger(red),
        // 그 외(빈 쿼리 / 정상 매치)는 muted. (디자인 search_bar.jsx:44-49)
        let has_query = !state.search.query.is_empty();
        let (counter_text, counter_color) = if state.search.matches.is_empty() {
            let color = if has_query {
                theme.accent_danger()
            } else {
                theme.text_muted()
            };
            ("0/0".to_string(), color)
        } else {
            let text = t("search.match_count")
                .replace("{current}", &(state.search.current_index + 1).to_string())
                .replace("{total}", &state.search.matches.len().to_string());
            (text, theme.text_muted())
        };
        draw_counter(ui, &counter_text, counter_color.into());

        // Prev/Next buttons — 항상 렌더, 매치가 없으면 disabled. (디자인 IconButton
        // size="sm", chevron SVG. search_bar.jsx:71-80)
        let nav_enabled = !state.search.matches.is_empty();
        if nav_button(
            ui,
            &theme,
            icons::CHEVRON_UP,
            nav_enabled,
            t("search.prev_match_tooltip"),
        ) {
            state.search.prev_match();
            scroll_to_current_match(state, engine);
        }
        if nav_button(
            ui,
            &theme,
            icons::CHEVRON_DOWN,
            nav_enabled,
            t("search.next_match_tooltip"),
        ) {
            state.search.next_match();
            scroll_to_current_match(state, engine);
        }

        // Option toggles: case / regex / whole-word.
        if toggle_button(
            ui,
            &theme,
            "Aa",
            !state.search.case_insensitive,
            t("search.case_tooltip"),
        ) {
            state.search.case_insensitive = !state.search.case_insensitive;
            run_search(state, engine);
        }
        if toggle_button(
            ui,
            &theme,
            ".*",
            state.search.regex,
            t("search.regex_tooltip"),
        ) {
            state.search.regex = !state.search.regex;
            run_search(state, engine);
        }
        if toggle_button(
            ui,
            &theme,
            "ab",
            state.search.whole_word,
            t("search.whole_word_tooltip"),
        ) {
            state.search.whole_word = !state.search.whole_word;
            run_search(state, engine);
        }

        // Divider + close — 토글 그룹과 close 버튼을 시각적으로 구분한다.
        // (디자인 search_bar.jsx: "· divider · ✕ close (Esc)")
        draw_divider(ui, &theme);
        if nav_button(ui, &theme, icons::CLOSE, true, t("search.close_tooltip")) {
            // X 클릭 = Escape 와 동일 동작: 검색 상태 clear + 팝업 닫기.
            state.search.clear();
            action = PopupAction::Close;
        }
    });

    action
}

/// 고정폭(40px) 매치 카운터를 가운데 정렬로 그린다. 텍스트 길이와 무관하게
/// 폭이 고정되어 옆의 ▲▼/토글이 좌우로 밀리지 않는다. (디자인 width:40, center)
fn draw_counter(ui: &mut egui::Ui, text: &str, color: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(40.0, ui.available_height()),
        egui::Sense::hover(),
    );
    let galley = ui.painter().layout_no_wrap(
        text.to_string(),
        egui::FontId::proportional(COUNTER_FONT_PRIMITIVE_12.value()),
        color,
    );
    let pos = rect.center() - galley.size() * 0.5;
    ui.painter().galley(pos, galley, color);
}

/// 토글 그룹과 close 버튼 사이 세로 구분선. (디자인 search_bar.jsx: divider,
/// 갤러리 specimen `catalog/components/search_bar.rs` 와 동일 규격)
fn draw_divider(ui: &mut egui::Ui, theme: &Theme) {
    let height = theme.item_height_tab.value() * 0.6;
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(theme.border_width.value(), height),
        egui::Sense::hover(),
    );
    ui.painter().vline(
        rect.center().x,
        rect.y_range(),
        egui::Stroke::new(
            theme.border_width.value(),
            egui::Color32::from(theme.separator),
        ),
    );
}

/// IconButton sm 규격의 정사각 슬롯을 할당하고 hover/active 배경 오버레이를
/// 그린다. `active` 면 토글-on 배경, `enabled=false` 면 상호작용 비활성(배경 없음).
/// rect 와 response 를 돌려준다.
fn icon_button_frame(
    ui: &mut egui::Ui,
    theme: &Theme,
    enabled: bool,
    active: bool,
) -> (egui::Rect, egui::Response) {
    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    // IconButton sm 정사각 프레임 = `--tasty-control-height-tab` (item_height_tab), 코너 = `--tasty-radius` (corner_radius).
    let btn_size = theme.item_height_tab.value();
    let radius = theme.corner_radius.value();
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(btn_size, btn_size), sense);
    if active || (enabled && resp.is_pointer_button_down_on()) {
        ui.painter()
            .rect_filled(rect, radius, theme.overlay_active().to_egui_premultiplied());
    } else if enabled && resp.hovered() {
        ui.painter()
            .rect_filled(rect, radius, theme.overlay_hover().to_egui_premultiplied());
    }
    (rect, resp)
}

/// chevron SVG 를 담은 IconButton sm — prev/next 네비게이션. `enabled=false` 면
/// opacity 0.45 로 흐리게 그리고 클릭을 받지 않는다. Returns true when clicked.
fn nav_button(
    ui: &mut egui::Ui,
    theme: &Theme,
    icon: icons::Icon,
    enabled: bool,
    tooltip: impl Into<egui::WidgetText>,
) -> bool {
    let (rect, resp) = icon_button_frame(ui, theme, enabled, false);
    // disabled 아이콘 버튼 톤. `opacity_disabled`(0.5)와 값이 다르다 — 이 자리를
    // 그 토큰으로 보낼지는 디자인 판단이라 값에 이름만 둔다.
    const ICON_BUTTON_DISABLED_OPACITY: f32 = 0.45;
    let color: egui::Color32 = if !enabled {
        egui::Color32::from(theme.text_secondary()).gamma_multiply(ICON_BUTTON_DISABLED_OPACITY)
    } else if resp.hovered() {
        theme.text_primary().into()
    } else {
        theme.text_secondary().into()
    };
    let glyph = theme.icon_glyph_size_sm.value();
    let icon_rect = egui::Rect::from_center_size(rect.center(), egui::vec2(glyph, glyph));
    icon.image(glyph, color).paint_at(ui, icon_rect);
    if enabled {
        resp.clone().on_hover_text(tooltip);
        resp.clicked()
    } else {
        false
    }
}

/// mono 라벨을 담은 IconButton sm 토글 — active 면 active_overlay 배경 + 라벨 색
/// text_primary, 비활성이면 배경 없음 + text_muted. Returns true when clicked.
fn toggle_button(
    ui: &mut egui::Ui,
    theme: &Theme,
    label: &str,
    active: bool,
    tooltip: impl Into<egui::WidgetText>,
) -> bool {
    let (rect, resp) = icon_button_frame(ui, theme, true, active);
    let color: egui::Color32 = if active {
        theme.text_primary().into()
    } else {
        theme.text_muted().into()
    };
    let galley = ui.painter().layout_no_wrap(
        label.to_string(),
        egui::FontId::monospace(theme.font_size_caption.value()),
        color,
    );
    let pos = rect.center() - galley.size() * 0.5;
    ui.painter().galley(pos, galley, color);
    resp.clone().on_hover_text(tooltip);
    resp.clicked()
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
