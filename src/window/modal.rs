use crate::window::{Modality, Window, WindowAction};

/// 모달 계열 윈도우가 공유하는 동작.
///
/// 모달 공통 특성:
/// - 엔진 전역에 최대 1개
/// - 활성 시 다른 윈도우의 입력을 차단
/// - 생성 직후엔 invisible, 첫 프레임 렌더 이후 show (깜빡임 방지)
/// - Esc 입력 시 기본적으로 닫힘
pub(crate) trait ModalWindow: Window {
    /// 첫 프레임이 렌더되었는지.
    fn shown(&self) -> bool;
    fn set_shown(&mut self, v: bool);

    /// 첫 렌더 후 윈도우를 가시화한다. 렌더 메서드 끝에서 호출한다.
    fn reveal_after_first_render(&mut self) {
        if !self.shown() {
            self.base().winit.set_visible(true);
            self.set_shown(true);
        }
    }

    /// Esc 키가 눌렸을 때의 기본 동작. 구현체가 override 가능.
    fn on_escape(&mut self) -> WindowAction {
        WindowAction::Close
    }
}

/// 모달 구현체가 `Window::modality`에 반환해야 하는 값.
/// (blanket impl로 자동화하면 BaseWindow와 겹치므로 각 구현체에서 한 줄 반환.)
///
/// quit/preset/plugins/settings 4개 modal 구현체가 사용. `Window::modality()`
/// trait dispatch가 호출 0이라 추적상 dead로 잡히지만 도메인 표현으로 보존.
#[allow(dead_code)]
pub(crate) const MODAL_MODALITY: Modality = Modality::Modal;
