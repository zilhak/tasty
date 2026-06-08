//! shortcuts 모듈 단위 테스트 — binding parsing/matching + zoom 단축키.

use winit::keyboard::{Key, ModifiersState, NamedKey, SmolStr};

use super::binding::{matches_binding, parse_binding};
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
    assert!(app.plugin_font_overrides.get("markdown").is_none());
    assert!(app.plugin_font_overrides.get("explorer").is_none());
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
