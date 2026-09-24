//! 포커스 전환 시 winit 이 합성한 키 이벤트 판정.
//!
//! X11·Windows에서 winit은 포커스를 얻거나 잃을 때 눌린 키의 Pressed/Released를
//! 합성한다. `is_synthetic == true`인 이벤트는 사용자 입력으로 처리하지 않는다.
//!
//! 정책과 근거: `docs/design/policies/key-mapping.md` 의 "합성 키 이벤트" 절.

use winit::event::WindowEvent;

/// 포커스 전환 때 합성한 키 이벤트인지 판별한다.
/// 호출부는 해당 이벤트를 단축키·PTY·egui 입력에 전달하지 않는다.
///
/// modifier는 별도의 ModifiersChanged로 갱신된다(X11: update_mods_from_query,
/// Windows: gain_active_focus의 update_modifiers). 합성하지 않는 macOS·Wayland에서는 false다.
pub fn is_synthetic_key_event(event: &WindowEvent) -> bool {
    matches!(
        event,
        WindowEvent::KeyboardInput {
            is_synthetic: true,
            ..
        }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 키보드 외 이벤트는 제외한다. KeyEvent의 비공개 필드 때문에 여기서는 키 이벤트를
    /// 직접 만들 수 없다. 합성 키 처리는 synthetic_key_event_guard.rs의 소스 검사와
    /// key-mapping 가이드에 기록된 X11 실측으로 확인한다.
    #[test]
    fn non_keyboard_events_pass_through() {
        assert!(!is_synthetic_key_event(&WindowEvent::Focused(true)));
        assert!(!is_synthetic_key_event(&WindowEvent::Focused(false)));
        assert!(!is_synthetic_key_event(&WindowEvent::RedrawRequested));
        assert!(!is_synthetic_key_event(&WindowEvent::CloseRequested));
    }
}
