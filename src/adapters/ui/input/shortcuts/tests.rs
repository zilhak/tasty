//! shortcuts 모듈 단위 테스트 — binding parsing/matching + zoom 단축키.

use winit::keyboard::{Key, KeyCode, ModifiersState, NamedKey, PhysicalKey, SmolStr};

use super::physical_key_to_logical;
use crate::view::main::MainView;
use tasty_key_match::{matches_binding, parse_binding};

// macOS에서는 설정의 alt가 Command에 대응하고, 다른 플랫폼에서는 Alt에 대응한다.
#[cfg(target_os = "macos")]
const BINDING_ALT: ModifiersState = ModifiersState::SUPER;
#[cfg(not(target_os = "macos"))]
const BINDING_ALT: ModifiersState = ModifiersState::ALT;

/// 바인딩 `option` 토큰에 대응하는 modifier — macOS 전용(Option = winit `ALT`).
#[cfg(target_os = "macos")]
const BINDING_OPTION: ModifiersState = ModifiersState::ALT;

fn mods_ctrl() -> ModifiersState {
    ModifiersState::CONTROL
}
fn mods_ctrl_shift() -> ModifiersState {
    ModifiersState::CONTROL | ModifiersState::SHIFT
}
fn mods_alt() -> ModifiersState {
    BINDING_ALT
}
fn mods_none() -> ModifiersState {
    ModifiersState::empty()
}
fn mods_ctrl_alt() -> ModifiersState {
    ModifiersState::CONTROL | BINDING_ALT
}
fn mods_alt_shift() -> ModifiersState {
    BINDING_ALT | ModifiersState::SHIFT
}
fn k_char(s: &str) -> Key {
    Key::Character(SmolStr::new(s))
}
fn k_named(n: NamedKey) -> Key {
    Key::Named(n)
}

#[test]
fn parse_simple_modifier_plus_key() {
    let p = parse_binding("ctrl+a").unwrap();
    assert!(p.ctrl && !p.shift && !p.alt);
    assert_eq!(p.key, "a");
}

#[test]
fn parse_double_plus_is_plus_key() {
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

#[test]
fn ctrl_alone_does_not_match_any_binding() {
    let key = k_named(NamedKey::Control);
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
    let key = k_char("+");
    assert!(!matches_binding("ctrl++", &key, mods_ctrl_shift()));
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
    let key = k_named(NamedKey::Control);
    assert!(!matches_binding("ctrl+a", &key, mods_ctrl()));
}

// IME가 논리 키를 바꿔도 physical_key로 US 키 위치를 판별한다.

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

// 실행 중인 플랫폼의 modifier 매핑을 검증한다.

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
    assert!(!app.plugin_font_overrides.contains_key("markdown"));
    assert!(!app.plugin_font_overrides.contains_key("explorer"));
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

// 사용자 설정과 무관하게 기본 quick-switch 설정을 사용한다.

fn add_test_workspace(state: &mut crate::state::AppState, engine: &mut crate::core::CoreState) {
    let event = crate::core::apply_create_workspace_inner(
        engine,
        crate::core::WorkspaceCreationParams::terminal(),
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
    state.add_tab(&mut engine).unwrap();
    state.add_tab(&mut engine).unwrap();
    state.goto_tab_in_pane(&mut engine, 0);
    let mut kb = crate::settings::KeybindingSettings::default();
    kb.set_tab_slot_key(2, "q");
    let consumed = MainView::handle_numeric_switch_shortcuts(
        &mut state,
        &mut engine,
        &kb,
        &k_char("q"),
        mods_ctrl(),
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
    assert!(MainView::handle_numeric_switch_shortcuts(
        &mut state,
        &mut engine,
        &kb,
        &k_char("l"),
        mods_ctrl(),
        true,
        false,
        false,
        false,
    ));
    assert_eq!(state.focused_pane(&engine).unwrap().active_tab, 1);
    assert!(MainView::handle_numeric_switch_shortcuts(
        &mut state,
        &mut engine,
        &kb,
        &k_char("h"),
        mods_ctrl(),
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
    assert!(MainView::handle_numeric_switch_shortcuts(
        &mut state,
        &mut engine,
        &kb,
        &k_char("j"),
        mods_alt(),
        false,
        false,
        true,  // alt
        false, // option
    ));
    assert_eq!(state.active_workspace, 1);
    assert!(MainView::handle_numeric_switch_shortcuts(
        &mut state,
        &mut engine,
        &kb,
        &k_char("k"),
        mods_alt(),
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
    assert!(MainView::handle_numeric_switch_shortcuts(
        &mut state,
        &mut engine,
        &kb,
        &k_char("2"),
        mods_alt(),
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
    assert!(!MainView::handle_numeric_switch_shortcuts(
        &mut state,
        &mut engine,
        &kb,
        &k_char("1"),
        mods_none(),
        false,
        false,
        false,
        false,
    ));
    assert!(!MainView::handle_numeric_switch_shortcuts(
        &mut state,
        &mut engine,
        &kb,
        &k_char("z"),
        mods_ctrl(),
        true,
        false,
        false,
        false,
    ));
    assert_eq!(state.focused_pane(&engine).unwrap().active_tab, before);
}

#[test]
fn category_combo_routes_to_category_switch() {
    let (mut state, mut engine) = fresh_state();
    engine.settings.general.workspace_categories_enabled = true;
    add_test_workspace(&mut state, &mut engine); // ws0 (normal)
    add_test_workspace(&mut state, &mut engine); // ws1
    let cat = engine.create_category("Services").unwrap();
    let ws1_id = engine.workspaces[1].id;
    engine.set_workspace_category(ws1_id, cat).unwrap();
    state.switch_workspace(&mut engine, 0); // active = ws0 (normal)
    let kb = crate::settings::KeybindingSettings::default(); // cat=ctrl+shift, slot "2"=섹션 index 1
    assert!(MainView::handle_numeric_switch_shortcuts(
        &mut state,
        &mut engine,
        &kb,
        &k_char("2"),
        mods_ctrl_shift(),
        true,  // ctrl
        true,  // shift
        false, // alt
        false, // option
    ));
    assert_eq!(state.active_workspace, 1);
}

#[test]
fn category_next_prev_keys_cycle_categories() {
    let (mut state, mut engine) = fresh_state();
    engine.settings.general.workspace_categories_enabled = true;
    add_test_workspace(&mut state, &mut engine); // ws1
    add_test_workspace(&mut state, &mut engine); // ws2
    let services = engine.create_category("Services").unwrap();
    let extra = engine.create_category("Extra").unwrap();
    let ws1_id = engine.workspaces[1].id;
    let ws2_id = engine.workspaces[2].id;
    engine.set_workspace_category(ws1_id, services).unwrap();
    engine.set_workspace_category(ws2_id, extra).unwrap();
    state.switch_workspace(&mut engine, 0); // active = ws0 (normal)
    let kb = crate::settings::KeybindingSettings::default();

    assert!(MainView::handle_numeric_switch_shortcuts(
        &mut state,
        &mut engine,
        &kb,
        &k_char("j"),
        mods_ctrl_shift(),
        true,
        true,
        false,
        false,
    ));
    assert_eq!(state.active_workspace, 1);
    assert!(MainView::handle_numeric_switch_shortcuts(
        &mut state,
        &mut engine,
        &kb,
        &k_char("j"),
        mods_ctrl_shift(),
        true,
        true,
        false,
        false,
    ));
    assert_eq!(state.active_workspace, 2);
    assert!(MainView::handle_numeric_switch_shortcuts(
        &mut state,
        &mut engine,
        &kb,
        &k_char("k"),
        mods_ctrl_shift(),
        true,
        true,
        false,
        false,
    ));
    assert_eq!(state.active_workspace, 1);
}

#[test]
fn category_next_prev_keys_noop_when_folders_disabled() {
    let (mut state, mut engine) = fresh_state();
    engine.settings.general.workspace_categories_enabled = false;
    add_test_workspace(&mut state, &mut engine);
    state.switch_workspace(&mut engine, 0);
    let kb = crate::settings::KeybindingSettings::default();
    assert!(!MainView::handle_numeric_switch_shortcuts(
        &mut state,
        &mut engine,
        &kb,
        &k_char("j"),
        mods_ctrl_shift(),
        true,
        true,
        false,
        false,
    ));
    assert_eq!(state.active_workspace, 0);
}

#[test]
fn individual_tab_axis_slot_and_next_prev_dispatch() {
    let (mut state, mut engine) = fresh_state();
    state.add_tab(&mut engine).unwrap();
    state.add_tab(&mut engine).unwrap(); // 3 tabs
    state.goto_tab_in_pane(&mut engine, 0);
    let mut kb = crate::settings::KeybindingSettings {
        tab_switch_modifier: crate::settings::KeybindingSettings::INDIVIDUAL_SWITCH_MODIFIER
            .to_string(),
        ..Default::default()
    };
    kb.set_tab_slot_key(2, "ctrl+alt+q"); // 3번째 탭 슬롯 = 완전 콤보.
    kb.set_tab_next_key("alt+shift+l");
    kb.set_tab_prev_key("alt+shift+h");

    assert!(MainView::handle_numeric_switch_shortcuts(
        &mut state,
        &mut engine,
        &kb,
        &k_char("q"),
        mods_ctrl_alt(),
        false,
        false,
        false,
        false,
    ));
    assert_eq!(state.focused_pane(&engine).unwrap().active_tab, 2);
    assert!(MainView::handle_numeric_switch_shortcuts(
        &mut state,
        &mut engine,
        &kb,
        &k_char("h"),
        mods_alt_shift(),
        false,
        true,
        true,
        false,
    ));
    assert_eq!(state.focused_pane(&engine).unwrap().active_tab, 1);
    // 개별 지정은 규칙 기반 대상 조회에서 제외한다.
    assert!(!MainView::handle_numeric_switch_shortcuts(
        &mut state,
        &mut engine,
        &kb,
        &k_char("1"),
        mods_ctrl(),
        true,
        false,
        false,
        false,
    ));
    assert_eq!(state.focused_pane(&engine).unwrap().active_tab, 1);
}

#[test]
fn individual_workspace_axis_slot_dispatch() {
    let (mut state, mut engine) = fresh_state();
    add_test_workspace(&mut state, &mut engine); // ws 1
    add_test_workspace(&mut state, &mut engine); // ws 2
    state.switch_workspace(&mut engine, 0);
    let mut kb = crate::settings::KeybindingSettings {
        workspace_switch_modifier: crate::settings::KeybindingSettings::INDIVIDUAL_SWITCH_MODIFIER
            .to_string(),
        ..Default::default()
    };
    kb.set_workspace_slot_key(1, "ctrl+alt+w"); // 2번째 워크스페이스(index 1) = 완전 콤보.

    assert!(MainView::handle_numeric_switch_shortcuts(
        &mut state,
        &mut engine,
        &kb,
        &k_char("w"),
        mods_ctrl_alt(),
        true,
        false,
        true,
        false,
    ));
    assert_eq!(state.active_workspace, 1);
}

#[test]
fn individual_category_axis_respects_folders_gate() {
    let (mut state, mut engine) = fresh_state();
    engine.settings.general.workspace_categories_enabled = true;
    add_test_workspace(&mut state, &mut engine); // ws0(normal) 이미 있으니 ws1 추가
    let cat = engine.create_category("Services").unwrap();
    let ws1_id = engine.workspaces[1].id;
    engine.set_workspace_category(ws1_id, cat).unwrap();
    state.switch_workspace(&mut engine, 0);
    let mut kb = crate::settings::KeybindingSettings {
        category_switch_modifier: crate::settings::KeybindingSettings::INDIVIDUAL_SWITCH_MODIFIER
            .to_string(),
        ..Default::default()
    };
    kb.category_switch_slot_keys[1] = "ctrl+alt+shift+s".to_string(); // 섹션 index 1.

    let mods = ModifiersState::CONTROL | BINDING_ALT | ModifiersState::SHIFT;
    assert!(MainView::handle_numeric_switch_shortcuts(
        &mut state,
        &mut engine,
        &kb,
        &k_char("s"),
        mods,
        true,
        true,
        true,
        false,
    ));
    assert_eq!(state.active_workspace, 1);

    state.switch_workspace(&mut engine, 0);
    engine.settings.general.workspace_categories_enabled = false;
    assert!(!MainView::handle_numeric_switch_shortcuts(
        &mut state,
        &mut engine,
        &kb,
        &k_char("s"),
        mods,
        true,
        true,
        true,
        false,
    ));
    assert_eq!(state.active_workspace, 0);
}

#[test]
fn axis_combos_do_not_cross_route() {
    let (mut state, mut engine) = fresh_state();
    state.add_tab(&mut engine).unwrap(); // 2 tabs
    state.goto_tab_in_pane(&mut engine, 0);
    let kb = crate::settings::KeybindingSettings::default();
    let before = state.focused_pane(&engine).unwrap().active_tab;
    assert!(!MainView::handle_numeric_switch_shortcuts(
        &mut state,
        &mut engine,
        &kb,
        &k_char("2"),
        mods_ctrl_shift(),
        true,  // ctrl
        true,  // shift
        false, // alt
        false, // option
    ));
    assert_eq!(state.focused_pane(&engine).unwrap().active_tab, before);
    assert!(MainView::handle_numeric_switch_shortcuts(
        &mut state,
        &mut engine,
        &kb,
        &k_char("2"),
        mods_ctrl(),
        true,  // ctrl
        false, // shift
        false, // alt
        false, // option
    ));
    assert_eq!(state.focused_pane(&engine).unwrap().active_tab, 1);
}

fn default_new_workspace_key_mods() -> (Key, ModifiersState) {
    let kb = crate::settings::KeybindingSettings::default();
    kb.new_workspace
        .first()
        .and_then(|b| tasty_key_match::parse_binding(b))
        .map(|p| {
            let mut mods = ModifiersState::empty();
            if p.ctrl {
                mods |= ModifiersState::CONTROL;
            }
            if p.shift {
                mods |= ModifiersState::SHIFT;
            }
            if p.alt {
                mods |= BINDING_ALT;
            }
            #[cfg(target_os = "macos")]
            if p.option {
                mods |= BINDING_OPTION;
            }
            (k_char(p.key), mods)
        })
        .expect("default new_workspace binding must parse")
}

#[test]
fn focused_workspace_category_returns_active_workspace_category() {
    let (mut state, mut engine) = fresh_state();
    let work = engine.create_category("Work").unwrap();
    add_test_workspace(&mut state, &mut engine); // ws1, 아직 normal
    let ws1_id = engine.workspaces[1].id;
    engine.set_workspace_category(ws1_id, work).unwrap();
    state.switch_workspace(&mut engine, 1);

    assert_eq!(
        super::focused_workspace_category(&state, &engine),
        Some(work)
    );
}

#[test]
fn focused_workspace_category_is_none_when_parked() {
    let (state, mut engine) = fresh_state();
    engine.workspaces.clear(); // parked 상태 (마지막 윈도우가 닫힌 뒤) 재현.
    assert_eq!(super::focused_workspace_category(&state, &engine), None);
}

#[test]
fn shortcut_new_workspace_inherits_active_category() {
    let (mut state, mut engine) = fresh_state();
    let work = engine.create_category("Work").unwrap();
    add_test_workspace(&mut state, &mut engine); // ws1
    let ws1_id = engine.workspaces[1].id;
    engine.set_workspace_category(ws1_id, work).unwrap();
    state.switch_workspace(&mut engine, 1);

    let kb = crate::settings::KeybindingSettings::default();
    let (key, mods) = default_new_workspace_key_mods();

    assert!(MainView::match_create_bindings(
        &mut state,
        &mut engine,
        &kb,
        &key,
        mods
    ));

    let intents = state.take_pending_intents();
    let category = intents.into_iter().find_map(|i| match i.body {
        crate::intent::Intent::NewWorkspace { category, .. } => Some(category),
        _ => None,
    });
    assert_eq!(category, Some(Some(work)));
}

#[test]
fn shortcut_new_workspace_stays_normal_when_categories_off() {
    // 카테고리가 꺼져 있으면 새 워크스페이스도 기본 카테고리를 사용한다.
    let (mut state, mut engine) = fresh_state();
    assert!(!engine.settings.general.workspace_categories_enabled);

    let kb = crate::settings::KeybindingSettings::default();
    let (key, mods) = default_new_workspace_key_mods();

    assert!(MainView::match_create_bindings(
        &mut state,
        &mut engine,
        &kb,
        &key,
        mods
    ));

    let intents = state.take_pending_intents();
    let category = intents.into_iter().find_map(|i| match i.body {
        crate::intent::Intent::NewWorkspace { category, .. } => Some(category),
        _ => None,
    });
    assert_eq!(category, Some(Some(crate::model::NORMAL_CATEGORY_ID)));
}

/// match arm에서 액션 문자열을 모은다. guard의 문자열은 액션 등록으로 세지 않는다.

/// `fn <fn_name>` 정의들 안의 `match <scrutinee> { … }` 팔 이름(따옴표 이름만).
fn match_arm_names(src: &str, fn_name: &str, scrutinee: &str) -> Vec<String> {
    use tasty_doc_guards::match_arms::{Source, matching_close};
    let s = Source::new(src);
    let bodies = s.fn_bodies(fn_name);
    assert!(!bodies.is_empty(), "`fn {fn_name}` 를 못 찾았다");
    let head = format!("match {scrutinee} {{");
    let mut out = Vec::new();
    for body in bodies {
        let mut from = body.start;
        while let Some(k) = s.code[from..body.end].find(&head) {
            let open = from + k + head.len() - 1;
            from = open + 1;
            let close = matching_close(&s.code, open).expect("match 블록이 닫히지 않는다");
            let arms = s
                .match_arms(open..close + 1)
                .unwrap_or_else(|e| panic!("`{fn_name}` 의 팔을 못 읽었다 — {e}"));
            for arm in arms {
                for alt in s.alternatives(&arm.pattern) {
                    if let Some(name) = s.plain_string(&alt) {
                        out.push(name.to_string());
                    }
                }
            }
        }
    }
    out
}

fn dispatchable_action_ids() -> Vec<String> {
    const SRC: &str = include_str!("dispatch.rs");
    match_arm_names(SRC, "dispatch_action_by_id", "action_id")
}

// 등록된 액션이 실행 경로에도 있는지 확인한다.
fn double_tap_registered_and_armed() -> (Vec<String>, Vec<String>) {
    const SRC: &str = include_str!("double_tap.rs");
    let list_start = SRC
        .find("let bindings_to_check")
        .expect("bindings_to_check 를 못 찾았다");
    let list_end = SRC[list_start..].find("];").expect("목록 끝") + list_start;
    let registered: Vec<String> = SRC[list_start..list_end]
        .lines()
        .filter_map(|l| {
            let l = l.trim();
            let rest = l.strip_prefix("(&kb.")?;
            let (_, after) = rest.split_once(", \"")?;
            Some(after.split('"').next()?.to_string())
        })
        .collect();

    // run_double_tap_ 접두어를 가진 실행 함수를 모두 읽는다.
    let code = tasty_doc_guards::source_text::mask_non_code(SRC);
    let mut names: Vec<&str> = code
        .match_indices("fn run_double_tap_")
        .map(|(at, _)| {
            let rest = &code[at + "fn ".len()..];
            let end = rest
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .unwrap_or(rest.len());
            &rest[..end]
        })
        .collect();
    names.sort_unstable();
    names.dedup();
    assert!(names.len() > 1, "실행 함수를 못 찾았다: {names:?}");
    let armed = names
        .iter()
        .flat_map(|name| match_arm_names(SRC, name, "action"))
        .collect();
    (registered, armed)
}

#[test]
fn every_listed_palette_action_has_an_execution_arm() {
    let runnable = dispatchable_action_ids();
    assert!(
        runnable.len() > 40,
        "arm 을 {}개밖에 못 읽었다 — 파싱이 깨졌다",
        runnable.len()
    );
    let missing: Vec<&str> = crate::state::command_palette::all_commands(&[])
        .iter()
        .filter_map(|c| match c {
            crate::state::command_palette::PaletteCommand::Host { id, .. } => Some(*id),
            crate::state::command_palette::PaletteCommand::Plugin { .. } => None,
        })
        .filter(|id| !runnable.iter().any(|r| r == id))
        .collect();
    assert!(missing.is_empty(), "팔레트에 뜨지만 실행 불가: {missing:?}");
}

#[test]
fn double_tap_registration_and_arms_agree() {
    let (registered, armed) = double_tap_registered_and_armed();
    assert!(
        registered.len() > 20 && armed.len() > 20,
        "등록 {} · arm {} — 파싱이 깨졌다",
        registered.len(),
        armed.len()
    );
    let unarmed: Vec<&String> = registered.iter().filter(|r| !armed.contains(r)).collect();
    let unregistered: Vec<&String> = armed.iter().filter(|a| !registered.contains(a)).collect();
    assert!(
        unarmed.is_empty() && unregistered.is_empty(),
        "등록됐는데 실행 arm 없음: {unarmed:?} · arm 인데 등록 안 됨: {unregistered:?}"
    );
}
