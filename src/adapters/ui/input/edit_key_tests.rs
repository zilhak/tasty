use super::*;
use winit::keyboard::SmolStr;

fn ch(c: &str) -> Key {
    Key::Character(SmolStr::new(c))
}

/// 설정의 `alt` 토큰. macOS에서는 Command다.
fn alt() -> ModifiersState {
    if cfg!(target_os = "macos") {
        ModifiersState::SUPER
    } else {
        ModifiersState::ALT
    }
}

fn kb(copy: &[&str], cut: &[&str], paste: &[&str]) -> crate::settings::KeybindingSettings {
    let v = |s: &[&str]| s.iter().map(|c| (*c).to_string()).collect();
    crate::settings::KeybindingSettings {
        copy: v(copy),
        cut: v(cut),
        paste: v(paste),
        ..Default::default()
    }
}

/// 바인딩을 옮기면 판정도 따라가고, 지운 키는 편집이 아니다.
#[test]
fn the_binding_follows_the_settings() {
    let settings = kb(&["alt+c"], &["alt+x"], &["alt+v"]);
    assert_eq!(
        edit_binding(&settings, &ch("c"), alt()),
        Some(EditKey::Copy)
    );
    assert_eq!(edit_binding(&settings, &ch("x"), alt()), Some(EditKey::Cut));
    assert_eq!(
        edit_binding(&settings, &ch("v"), alt()),
        Some(EditKey::Paste)
    );
    assert_eq!(edit_binding(&settings, &ch("q"), alt()), None);
    let removed = kb(&[], &[], &["alt+v"]);
    assert_eq!(edit_binding(&removed, &ch("c"), alt()), None);
}

/// 한 키를 여러 편집에 묶으면 복사 → 잘라내기 → 붙여넣기 순서로 하나만 고른다.
#[test]
fn an_overlapping_key_picks_one_edit_in_order() {
    let settings = kb(&["alt+k"], &["alt+k"], &["alt+k"]);
    assert_eq!(
        edit_binding(&settings, &ch("k"), alt()),
        Some(EditKey::Copy)
    );
    let settings = kb(&[], &["alt+k"], &["alt+k"]);
    assert_eq!(edit_binding(&settings, &ch("k"), alt()), Some(EditKey::Cut));
}
