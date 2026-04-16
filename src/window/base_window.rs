use crate::window::{Modality, Window};

/// 일반(비모달) 윈도우 계열이 공유하는 동작.
///
/// 현재는 `MainWindow`만 해당. 미래에 `StandaloneSurfaceWindow`,
/// `StandaloneWorkspaceWindow` 등이 여기에 추가된다.
///
/// 공통 특성:
/// - OS 네이티브 포커스에 독립적으로 참여
/// - 모달이 활성 상태일 때만 입력 차단됨
pub trait BaseWindow: Window {
    /// 사이드바를 가지는지 (기본 true). StandaloneSurface는 override.
    fn has_sidebar(&self) -> bool {
        true
    }
}

/// 일반 윈도우 구현체가 `Window::modality`에 반환해야 하는 값.
pub const MODELESS_MODALITY: Modality = Modality::Modeless;
