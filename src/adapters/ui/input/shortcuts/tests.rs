//! shortcuts 모듈 단위 테스트 — binding parsing/matching + zoom 단축키.

use winit::keyboard::{Key, KeyCode, ModifiersState, NamedKey, PhysicalKey, SmolStr};

use super::binding::{matches_binding, parse_binding};
use super::physical_key_to_logical;
use crate::view::main::MainView;

fn mods_ctrl() -> ModifiersState {
    ModifiersState::CONTROL
}
fn mods_ctrl_shift() -> ModifiersState {
    ModifiersState::CONTROL | ModifiersState::SHIFT
}
fn mods_none() -> ModifiersState {
    ModifiersState::empty()
}
fn k_char(s: &str) -> Key {
    Key::Character(SmolStr::new(s))
}
fn k_named(n: NamedKey) -> Key {
    Key::Named(n)
}

// ── parse_binding 동작 ────────────────────────────────────────────

#[test]
fn parse_simple_modifier_plus_key() {
    let p = parse_binding("ctrl+a").unwrap();
    assert!(p.ctrl && !p.shift && !p.alt);
    assert_eq!(p.key, "a");
}

#[test]
fn parse_double_plus_is_plus_key() {
    // "ctrl++" = Ctrl + `+` 키.
    let p = parse_binding("ctrl++").unwrap();
    assert!(p.ctrl && !p.shift && !p.alt);
    assert_eq!(p.key, "+");
}

#[test]
fn parse_minus_and_equals() {
    assert_eq!(parse_binding("ctrl+-").unwrap().key, "-");
    assert_eq!(parse_binding("ctrl+=").unwrap().key, "=");
}

#[test]
fn parse_plus_alias_is_canonical() {
    let p = parse_binding("ctrl+plus").unwrap();
    assert_eq!(p.key, "plus");
}

#[test]
fn parse_empty_is_rejected() {
    assert!(parse_binding("").is_none());
}

#[test]
fn parse_trailing_plus_is_rejected() {
    // "ctrl+"처럼 키가 없는 경우.
    assert!(parse_binding("ctrl+").is_none());
}

#[test]
fn parse_modifier_only_is_rejected() {
    assert!(parse_binding("ctrl").is_none());
    assert!(parse_binding("shift").is_none());
    assert!(parse_binding("alt").is_none());
}

#[test]
fn parse_accepts_any_modifier_order() {
    let p1 = parse_binding("ctrl+shift+a").unwrap();
    let p2 = parse_binding("shift+ctrl+a").unwrap();
    assert_eq!((p1.ctrl, p1.shift, p1.key), (true, true, "a"));
    assert_eq!((p2.ctrl, p2.shift, p2.key), (true, true, "a"));
}

#[test]
fn parse_is_case_insensitive_for_modifiers() {
    let p = parse_binding("CTRL+A").unwrap();
    assert!(p.ctrl);
    assert_eq!(p.key, "A");
}

// ── matches_binding: 모디파이어 단독 방어 ─────────────────────────

#[test]
fn ctrl_alone_does_not_match_any_binding() {
    let key = k_named(NamedKey::Control);
    // 어떤 바인딩과도 Ctrl 단독은 매칭되지 않아야 한다.
    for binding in ["ctrl++", "ctrl+=", "ctrl+plus", "ctrl+a", "ctrl+shift+="] {
        assert!(
            !matches_binding(binding, &key, mods_ctrl()),
            "binding {binding:?}가 Ctrl 단독에 매칭되면 안 된다"
        );
    }
}

#[test]
fn shift_alone_does_not_match_any_binding() {
    let key = k_named(NamedKey::Shift);
    assert!(!matches_binding("shift+a", &key, ModifiersState::SHIFT));
}

#[test]
fn alt_alone_does_not_match_any_binding() {
    let key = k_named(NamedKey::Alt);
    assert!(!matches_binding("alt+a", &key, ModifiersState::ALT));
}

// ── matches_binding: 정상 매칭 경로 ───────────────────────────────

#[test]
fn plus_key_matches_ctrl_plus_binding() {
    let key = k_char("+");
    assert!(matches_binding("ctrl++", &key, mods_ctrl()));
}

#[test]
fn plus_alias_matches_plus_character() {
    let key = k_char("+");
    assert!(matches_binding("ctrl+plus", &key, mods_ctrl()));
}

#[test]
fn plus_character_matches_plus_alias_and_literal() {
    let key = k_char("+");
    assert!(matches_binding("ctrl+plus", &key, mods_ctrl()));
    assert!(matches_binding("ctrl++", &key, mods_ctrl()));
}

#[test]
fn equals_key_matches_ctrl_equals_binding() {
    let key = k_char("=");
    assert!(matches_binding("ctrl+=", &key, mods_ctrl()));
    assert!(matches_binding("ctrl+equals", &key, mods_ctrl()));
}

#[test]
fn minus_key_matches_ctrl_minus_binding() {
    let key = k_char("-");
    assert!(matches_binding("ctrl+-", &key, mods_ctrl()));
    assert!(matches_binding("ctrl+minus", &key, mods_ctrl()));
}

#[test]
fn shift_requirement_is_enforced() {
    // "ctrl++"는 Shift를 기대하지 않으므로 Ctrl+Shift+<+키>는 매칭 안 됨.
    let key = k_char("+");
    assert!(!matches_binding("ctrl++", &key, mods_ctrl_shift()));
    // 반대로 "ctrl+shift+="는 shift를 요구.
    let eq = k_char("=");
    assert!(matches_binding("ctrl+shift+=", &eq, mods_ctrl_shift()));
    assert!(!matches_binding("ctrl+shift+=", &eq, mods_ctrl()));
}

#[test]
fn letter_matches_both_char_and_control_char() {
    // Ctrl+letter가 0x01-0x1A로 도착해도 매칭.
    let ctrl_a = k_char("\u{1}"); // Ctrl+A = 0x01
    assert!(matches_binding("ctrl+a", &ctrl_a, mods_ctrl()));
    let plain_a = k_char("a");
    assert!(matches_binding("ctrl+a", &plain_a, mods_ctrl()));
}

#[test]
fn no_modifier_binding_does_not_match_when_ctrl_held() {
    // 가상의 "a" 단독 바인딩 (파서는 허용하지만 의미상 수정자 요구 안 함).
    // Ctrl을 누르고 a를 눌렀는데 바인딩이 "a"뿐이라면 매칭되면 안 됨.
    let key = k_char("a");
    assert!(matches_binding("a", &key, mods_none()));
    assert!(!matches_binding("a", &key, mods_ctrl()));
}

#[test]
fn empty_binding_never_matches() {
    let key = k_char("a");
    assert!(!matches_binding("", &key, mods_none()));
}

#[test]
fn named_key_without_mapping_never_matches_empty() {
    // NamedKey::Control 같이 매핑이 없는 키는 매칭되지 않아야 한다.
    // 과거에는 named_str이 "" 를 반환해서 빈 key_part와 매칭되는 버그가 있었다.
    let key = k_named(NamedKey::Control);
    assert!(!matches_binding("ctrl+a", &key, mods_ctrl()));
}

// ── physical_key_to_logical: IME 조합 중 modifier 폴백 매핑 ─────────
//
// modifier(Ctrl/Cmd/Alt) 가 눌린 동안 IME 가 logical_key 를 조합문자로 덮어써도
// physical key code 로부터 US 레이아웃 기준 base 문자를 복원한다. 이 매핑이
// handle_keyboard_input 의 shortcut_lookup_key / terminal_key / vi_key 폴백을
// 뒷받침한다 — Ctrl+letter, 단축키 매칭, vi 키가 조합문자에 오염되지 않게 한다.

fn code(c: KeyCode) -> PhysicalKey {
    PhysicalKey::Code(c)
}

#[test]
fn physical_letters_map_to_lowercase_char() {
    for (kc, expected) in [
        (KeyCode::KeyA, "a"),
        (KeyCode::KeyC, "c"),
        (KeyCode::KeyM, "m"),
        (KeyCode::KeyZ, "z"),
    ] {
        assert_eq!(
            physical_key_to_logical(&code(kc)),
            Some(Key::Character(expected.into())),
            "{kc:?} should map to {expected:?}"
        );
    }
}

#[test]
fn physical_digits_map_to_digit_char() {
    for (kc, expected) in [
        (KeyCode::Digit0, "0"),
        (KeyCode::Digit1, "1"),
        (KeyCode::Digit9, "9"),
    ] {
        assert_eq!(
            physical_key_to_logical(&code(kc)),
            Some(Key::Character(expected.into())),
            "{kc:?} should map to {expected:?}"
        );
    }
}

#[test]
fn physical_punctuation_maps_to_symbol_char() {
    // zoom 단축키(=/-) 등에 쓰이는 기호가 조합 중에도 복원되는지.
    for (kc, expected) in [
        (KeyCode::Minus, "-"),
        (KeyCode::Equal, "="),
        (KeyCode::Slash, "/"),
        (KeyCode::Backslash, "\\"),
    ] {
        assert_eq!(
            physical_key_to_logical(&code(kc)),
            Some(Key::Character(expected.into())),
            "{kc:?} should map to {expected:?}"
        );
    }
}

#[test]
fn non_character_physical_keys_return_none() {
    // 글자/숫자/기호가 아닌 코드는 None → 호출부가 logical_key 로 폴백한다.
    assert_eq!(physical_key_to_logical(&code(KeyCode::Enter)), None);
    assert_eq!(physical_key_to_logical(&code(KeyCode::Space)), None);
    assert_eq!(physical_key_to_logical(&code(KeyCode::F1)), None);
    assert_eq!(physical_key_to_logical(&code(KeyCode::ArrowUp)), None);
}

#[test]
fn unidentified_physical_key_returns_none() {
    use winit::keyboard::NativeKeyCode;
    assert_eq!(
        physical_key_to_logical(&PhysicalKey::Unidentified(NativeKeyCode::Unidentified)),
        None
    );
}

// ── matches_binding: 플랫폼 alt/option modifier 매핑 ────────────────
//
// 비-macOS: 바인딩 "alt" → winit alt_key, "option" 바인딩은 절대 불일치.
// (macOS 분기는 이 타깃에서 컴파일되지 않으므로 non-macOS 규칙만 검증.)

#[test]
#[cfg(not(target_os = "macos"))]
fn alt_binding_matches_alt_modifier_on_non_macos() {
    let key = k_char("t");
    assert!(matches_binding("alt+t", &key, ModifiersState::ALT));
    assert!(!matches_binding("alt+t", &key, mods_none()));
    assert!(!matches_binding("t", &key, ModifiersState::ALT));
}

#[test]
#[cfg(not(target_os = "macos"))]
fn option_binding_never_matches_on_non_macos() {
    let key = k_char("t");
    assert!(!matches_binding("option+t", &key, ModifiersState::ALT));
    assert!(!matches_binding("option+t", &key, mods_none()));
}

// ── handle_zoom_shortcut: surface별 override 갱신 ──────────────────

fn fresh_state() -> (crate::state::AppState, crate::core::CoreState) {
    let waker: crate::terminal::Waker = std::sync::Arc::new(|| {});
    let mut engine = crate::core::CoreState::new(80, 24, waker).unwrap();
    let preset_store = std::sync::Arc::new(std::sync::Mutex::new(
        tasty_presets::PresetStore::load_default(),
    ));
    let memory: std::sync::Arc<std::sync::Mutex<dyn tasty_memory::MemoryStorage>> =
        std::sync::Arc::new(std::sync::Mutex::new(
            tasty_memory::testing::InMemoryStorage::new(),
        ));
    let state = crate::state::AppState::new(&mut engine, preset_store, memory);
    (state, engine)
}

#[test]
fn zoom_in_increments_terminal_font_size_override_only() {
    let (mut state, mut engine) = fresh_state();
    // Pin the default so the test is independent of the user's settings file.
    engine.settings.appearance.default_font.font_size = 14.0;
    engine.settings.appearance.terminal_font.font_size = None;
    engine.settings.appearance.plugin_font_overrides.clear();
    let consumed = MainView::handle_zoom_shortcut(
        &mut state,
        &mut engine,
        &k_char("="),
        ModifiersState::CONTROL,
    );
    assert!(consumed);
    let app = &engine.settings.appearance;
    assert_eq!(app.terminal_font.font_size, Some(15.0));
    // Other surfaces remain untouched.
    assert!(!app.plugin_font_overrides.contains_key("markdown"));
    assert!(!app.plugin_font_overrides.contains_key("explorer"));
    // default_font is also untouched.
    assert_eq!(app.default_font.font_size, 14.0);
}

#[test]
fn zoom_out_decrements_terminal_font_size_override() {
    let (mut state, mut engine) = fresh_state();
    engine.settings.appearance.terminal_font.font_size = Some(20.0);
    let consumed = MainView::handle_zoom_shortcut(
        &mut state,
        &mut engine,
        &k_char("-"),
        ModifiersState::CONTROL,
    );
    assert!(consumed);
    assert_eq!(
        engine.settings.appearance.terminal_font.font_size,
        Some(19.0)
    );
}

#[test]
fn zoom_reset_clears_terminal_font_size_override() {
    let (mut state, mut engine) = fresh_state();
    engine.settings.appearance.terminal_font.font_size = Some(20.0);
    let consumed = MainView::handle_zoom_shortcut(
        &mut state,
        &mut engine,
        &k_char("0"),
        ModifiersState::CONTROL,
    );
    assert!(consumed);
    // Reset → override removed (surface returns to default_font).
    assert!(engine.settings.appearance.terminal_font.font_size.is_none());
}

#[test]
fn zoom_in_clamps_at_72px() {
    let (mut state, mut engine) = fresh_state();
    engine.settings.appearance.terminal_font.font_size = Some(71.5);
    MainView::handle_zoom_shortcut(
        &mut state,
        &mut engine,
        &k_char("="),
        ModifiersState::CONTROL,
    );
    assert_eq!(
        engine.settings.appearance.terminal_font.font_size,
        Some(72.0)
    );
}

#[test]
fn zoom_out_clamps_at_6px() {
    let (mut state, mut engine) = fresh_state();
    engine.settings.appearance.terminal_font.font_size = Some(6.5);
    MainView::handle_zoom_shortcut(
        &mut state,
        &mut engine,
        &k_char("-"),
        ModifiersState::CONTROL,
    );
    assert_eq!(
        engine.settings.appearance.terminal_font.font_size,
        Some(6.0)
    );
}

// ── handle_numeric_switch_shortcuts: quick-switch 슬롯/next/prev 배선 (QS03) ──
//
// 기본 프리셋: tab modifier=ctrl, workspace modifier=alt, tab next/prev="l"/"h",
// workspace next/prev="j"/"k", workspace_categories_enabled=false.

fn add_test_workspace(state: &mut crate::state::AppState, engine: &mut crate::core::CoreState) {
    let event = crate::core::apply_create_workspace_inner(
        engine,
        None,
        "terminal".to_string(),
        serde_json::Value::Null,
        None,
        None,
        None,
        None,
    )
    .unwrap();
    let crate::core::intent::CoreEvent::WorkspaceCreated { index, .. } = event else {
        panic!("apply_create_workspace_inner did not return WorkspaceCreated");
    };
    state.active_workspace = index;
}

#[test]
fn custom_tab_slot_key_switches_correct_tab() {
    let (mut state, mut engine) = fresh_state();
    // focused pane 에 탭 3개 확보 (초기 1 + 2).
    state.add_tab(&mut engine).unwrap();
    state.add_tab(&mut engine).unwrap();
    state.goto_tab_in_pane(&mut engine, 0);
    // 3번째 슬롯(index 2)을 "q" 로 재바인딩.
    let mut kb = crate::settings::KeybindingSettings::default();
    kb.set_tab_slot_key(2, "q");
    // ctrl(기본 tab modifier) + "q" → focused pane 의 3번째 탭(index 2)으로 전환.
    let consumed = MainView::handle_numeric_switch_shortcuts(
        &mut state,
        &mut engine,
        &kb,
        &k_char("q"),
        true,  // ctrl
        false, // shift
        false, // alt
        false, // option
    );
    assert!(consumed);
    assert_eq!(state.focused_pane(&engine).unwrap().active_tab, 2);
}

#[test]
fn tab_next_prev_keys_cycle_focused_pane_tabs() {
    let (mut state, mut engine) = fresh_state();
    state.add_tab(&mut engine).unwrap();
    state.add_tab(&mut engine).unwrap(); // 3 tabs
    state.goto_tab_in_pane(&mut engine, 0);
    let kb = crate::settings::KeybindingSettings::default(); // next="l", prev="h", modifier ctrl
    // ctrl+l → 다음 탭.
    assert!(MainView::handle_numeric_switch_shortcuts(
        &mut state,
        &mut engine,
        &kb,
        &k_char("l"),
        true,
        false,
        false,
        false,
    ));
    assert_eq!(state.focused_pane(&engine).unwrap().active_tab, 1);
    // ctrl+h → 이전 탭.
    assert!(MainView::handle_numeric_switch_shortcuts(
        &mut state,
        &mut engine,
        &kb,
        &k_char("h"),
        true,
        false,
        false,
        false,
    ));
    assert_eq!(state.focused_pane(&engine).unwrap().active_tab, 0);
}

#[test]
fn workspace_next_prev_keys_trigger_category_switch() {
    let (mut state, mut engine) = fresh_state();
    add_test_workspace(&mut state, &mut engine); // ws 1
    add_test_workspace(&mut state, &mut engine); // ws 2
    state.switch_workspace(&mut engine, 0);
    let kb = crate::settings::KeybindingSettings::default(); // next="j", prev="k", modifier alt
    // alt+j → 같은 카테고리(기본 전부 normal) 내 다음 워크스페이스.
    assert!(MainView::handle_numeric_switch_shortcuts(
        &mut state,
        &mut engine,
        &kb,
        &k_char("j"),
        false,
        false,
        true,  // alt
        false, // option
    ));
    assert_eq!(state.active_workspace, 1);
    // alt+k → 이전.
    assert!(MainView::handle_numeric_switch_shortcuts(
        &mut state,
        &mut engine,
        &kb,
        &k_char("k"),
        false,
        false,
        true,
        false,
    ));
    assert_eq!(state.active_workspace, 0);
}

#[test]
fn workspace_slot_key_switches_workspace() {
    let (mut state, mut engine) = fresh_state();
    add_test_workspace(&mut state, &mut engine); // ws 1
    add_test_workspace(&mut state, &mut engine); // ws 2
    state.switch_workspace(&mut engine, 0);
    let kb = crate::settings::KeybindingSettings::default(); // slot "2" = index 1
    // alt+"2" → 2번째 워크스페이스(index 1). 카테고리 off → 전역 인덱스.
    assert!(MainView::handle_numeric_switch_shortcuts(
        &mut state,
        &mut engine,
        &kb,
        &k_char("2"),
        false,
        false,
        true,
        false,
    ));
    assert_eq!(state.active_workspace, 1);
}

#[test]
fn wrong_modifier_and_unbound_key_return_false() {
    let (mut state, mut engine) = fresh_state();
    state.add_tab(&mut engine).unwrap(); // 2 tabs
    state.goto_tab_in_pane(&mut engine, 0);
    let kb = crate::settings::KeybindingSettings::default();
    let before = state.focused_pane(&engine).unwrap().active_tab;
    // modifier 없이 "1" → 대상 판정 None → false (맨 키 오검출 없음).
    assert!(!MainView::handle_numeric_switch_shortcuts(
        &mut state,
        &mut engine,
        &kb,
        &k_char("1"),
        false,
        false,
        false,
        false,
    ));
    // ctrl + "z"(어떤 슬롯/next/prev 도 아님) → false.
    assert!(!MainView::handle_numeric_switch_shortcuts(
        &mut state,
        &mut engine,
        &kb,
        &k_char("z"),
        true,
        false,
        false,
        false,
    ));
    assert_eq!(state.focused_pane(&engine).unwrap().active_tab, before);
}

#[test]
fn category_combo_routes_to_category_switch() {
    // 기본 카테고리 modifier = ctrl+shift(독립 축). ctrl+shift+숫자 → 카테고리 전환.
    let (mut state, mut engine) = fresh_state();
    engine.settings.general.workspace_categories_enabled = true;
    add_test_workspace(&mut state, &mut engine); // ws0 (normal)
    add_test_workspace(&mut state, &mut engine); // ws1
    let cat = engine.create_category("Services").unwrap();
    let ws1_id = engine.workspaces[1].id;
    engine.set_workspace_category(ws1_id, cat).unwrap();
    state.switch_workspace(&mut engine, 0); // active = ws0 (normal)
    let kb = crate::settings::KeybindingSettings::default(); // cat=ctrl+shift, slot "2"=섹션 index 1
    // ctrl+shift+"2" → 섹션 index 1(Services) 로 카테고리 전환 → ws1 착지.
    assert!(MainView::handle_numeric_switch_shortcuts(
        &mut state,
        &mut engine,
        &kb,
        &k_char("2"),
        true,  // ctrl
        true,  // shift
        false, // alt
        false, // option
    ));
    assert_eq!(state.active_workspace, 1);
}

#[test]
fn axis_combos_do_not_cross_route() {
    // ctrl 단독 → Tab, ctrl+shift(카테고리 축) → 탭/워크스페이스로 새지 않음.
    let (mut state, mut engine) = fresh_state();
    state.add_tab(&mut engine).unwrap(); // 2 tabs
    state.goto_tab_in_pane(&mut engine, 0);
    let kb = crate::settings::KeybindingSettings::default();
    let before = state.focused_pane(&engine).unwrap().active_tab;
    // ctrl+shift+"2": categories off → Category 대상이지만 비소비(false), 탭 전환 없음.
    assert!(!MainView::handle_numeric_switch_shortcuts(
        &mut state,
        &mut engine,
        &kb,
        &k_char("2"),
        true,  // ctrl
        true,  // shift
        false, // alt
        false, // option
    ));
    assert_eq!(state.focused_pane(&engine).unwrap().active_tab, before);
    // ctrl 단독+"2" → 탭 전환 정상(2번째 탭 = index 1).
    assert!(MainView::handle_numeric_switch_shortcuts(
        &mut state,
        &mut engine,
        &kb,
        &k_char("2"),
        true,  // ctrl
        false, // shift
        false, // alt
        false, // option
    ));
    assert_eq!(state.focused_pane(&engine).unwrap().active_tab, 1);
}
