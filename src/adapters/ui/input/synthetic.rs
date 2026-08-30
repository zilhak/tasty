//! 포커스 전환 시 winit 이 합성한 키 이벤트 판정.
//!
//! winit 은 창이 포커스를 **얻는** 순간 그 시점에 물리적으로 눌려 있는 모든 키에 대해
//! `Pressed` 를, **잃는** 순간 같은 키들에 대해 `Released` 를 합성해 보낸다
//! (`WindowEvent::KeyboardInput::is_synthetic == true`, X11·Windows 한정). 사용자가 이 창
//! 안에서 누른 적이 없는 키이므로 tasty 는 이를 사용자 입력으로 취급하지 않는다.
//!
//! 정책과 근거: `docs/design/policies/key-mapping.md` 의 "합성 키 이벤트" 절.

use winit::event::WindowEvent;

/// winit 이 포커스 전환 시점에 합성한 키 이벤트인가.
///
/// `true` 면 호출부는 그 이벤트를 **버린다** — 단축키 매칭 · PTY 포워딩 · egui 입력
/// 어디에도 흘리지 않는다. 합성이 아닌 키 이벤트(`is_synthetic == false`)와 키보드가
/// 아닌 이벤트는 전부 `false` 라 평소 경로를 그대로 탄다.
///
/// 합성 이벤트를 통째로 버려도 modifier 상태는 깨지지 않는다. 양쪽 백엔드 모두 합성
/// 키와 **별개로** `ModifiersChanged` 를 보내기 때문이다 — X11 은 포커스 획득 처리
/// 말미의 `update_mods_from_query`, Windows 는 `gain_active_focus` 의
/// `update_modifiers` 가 담당한다. 합성을 하지 않는 macOS·Wayland 에서는 이 함수가
/// 항상 `false` 를 돌려주므로 동작이 달라지지 않는다 — `#[cfg]` 분기가 필요 없는 이유다.
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

    /// 키보드가 아닌 이벤트는 판정 대상이 아니다 — 포커스·리드로우 등 나머지 이벤트가
    /// 이 게이트에 걸려 사라지면 창이 통째로 먹통이 된다.
    ///
    /// `WindowEvent::KeyboardInput` 자체를 만드는 케이스는 여기 둘 수 없다.
    /// `KeyEvent::platform_specific` 이 winit `pub(crate)` 라 크레이트 밖에서는
    /// `KeyEvent` 를 구성할 수 없기 때문이다. 합성/실입력 대조는
    /// `tests/synthetic_key_event_guard.rs` 의 진입부 가드 트립와이어와 X11 실측
    /// (`docs/design/policies/key-mapping.md`) 이 담당한다.
    #[test]
    fn non_keyboard_events_pass_through() {
        assert!(!is_synthetic_key_event(&WindowEvent::Focused(true)));
        assert!(!is_synthetic_key_event(&WindowEvent::Focused(false)));
        assert!(!is_synthetic_key_event(&WindowEvent::RedrawRequested));
        assert!(!is_synthetic_key_event(&WindowEvent::CloseRequested));
    }
}
