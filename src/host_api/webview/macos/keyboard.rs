//! Native key conversion shared by the platform callbacks.

/// macOS의 레이아웃 독립 keyCode를 winit 물리 키로 변환한다.
pub(super) fn macos_keycode_to_physical(key_code: u16) -> winit::keyboard::PhysicalKey {
    use winit::platform::scancode::PhysicalKeyExtScancode;
    winit::keyboard::PhysicalKey::from_scancode(key_code as u32)
}

/// Command는 SUPER, Option은 ALT로 전달한다. 바인딩 토큰의 해석은 공통 matcher가 맡는다.
pub(super) fn nsevent_mods_to_winit(
    flags: objc2_app_kit::NSEventModifierFlags,
) -> winit::keyboard::ModifiersState {
    use objc2_app_kit::NSEventModifierFlags;
    use winit::keyboard::ModifiersState;
    let mut mods = ModifiersState::empty();
    mods.set(
        ModifiersState::CONTROL,
        flags.contains(NSEventModifierFlags::Control),
    );
    mods.set(
        ModifiersState::SHIFT,
        flags.contains(NSEventModifierFlags::Shift),
    );
    mods.set(
        ModifiersState::ALT,
        flags.contains(NSEventModifierFlags::Option),
    );
    mods.set(
        ModifiersState::SUPER,
        flags.contains(NSEventModifierFlags::Command),
    );
    mods
}

/// AppKit의 private-use named key와 문자를 winit 키로 바꾼다. 빈 문자열은 None이다.
pub(super) fn ns_chars_to_winit_key(chars: &str) -> Option<winit::keyboard::Key> {
    use winit::keyboard::{Key, NamedKey};
    let c = chars.chars().next()?;
    let named = match c as u32 {
        0xF700 => NamedKey::ArrowUp,
        0xF701 => NamedKey::ArrowDown,
        0xF702 => NamedKey::ArrowLeft,
        0xF703 => NamedKey::ArrowRight,
        0xF704 => NamedKey::F1,
        0xF705 => NamedKey::F2,
        0xF706 => NamedKey::F3,
        0xF707 => NamedKey::F4,
        0xF708 => NamedKey::F5,
        0xF709 => NamedKey::F6,
        0xF70A => NamedKey::F7,
        0xF70B => NamedKey::F8,
        0xF70C => NamedKey::F9,
        0xF70D => NamedKey::F10,
        0xF70E => NamedKey::F11,
        0xF70F => NamedKey::F12,
        0xF710 => NamedKey::F13,
        0xF711 => NamedKey::F14,
        0xF712 => NamedKey::F15,
        0xF713 => NamedKey::F16,
        0xF714 => NamedKey::F17,
        0xF715 => NamedKey::F18,
        0xF716 => NamedKey::F19,
        0xF717 => NamedKey::F20,
        0xF718 => NamedKey::F21,
        0xF719 => NamedKey::F22,
        0xF71A => NamedKey::F23,
        0xF71B => NamedKey::F24,
        0xF727 => NamedKey::Insert,
        0xF728 => NamedKey::Delete,
        0xF729 => NamedKey::Home,
        0xF72B => NamedKey::End,
        0xF72C => NamedKey::PageUp,
        0xF72D => NamedKey::PageDown,
        0x0D | 0x03 => NamedKey::Enter,
        0x09 => NamedKey::Tab,
        0x7F | 0x08 => NamedKey::Backspace,
        0x1B => NamedKey::Escape,
        0x20 => NamedKey::Space,
        _ => {
            if c.is_control() {
                return None;
            }
            return Some(Key::Character(c.to_string().into()));
        }
    };
    Some(Key::Named(named))
}
