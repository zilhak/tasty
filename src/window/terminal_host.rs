use crate::window::{Modality, Window};

/// 터미널 계열 콘텐츠를 호스팅하는 윈도우가 공유하는 동작.
///
/// "내부에 터미널 계열 Surface(Terminal / Markdown / Explorer / Html / Empty)가
/// 들어갈 수 있는 윈도우"라는 의미. 모달이 아닌 모든 일반 윈도우의 공통 계열.
///
/// 현재는 `MainWindow`(워크스페이스/사이드바/탭을 가진 메인 터미널 윈도우)만 해당.
/// 미래에 `StandaloneSurfaceWindow`(독립 Surface 하나만 가진 윈도우),
/// `StandaloneWorkspaceWindow`(워크스페이스 1개 고정) 등이 여기에 추가된다.
///
/// 공통 특성:
/// - OS 네이티브 포커스에 독립적으로 참여
/// - 모달이 활성 상태일 때만 입력 차단됨
/// - 내부에 Surface 트리를 호스팅 (개수/구조는 구현체마다 상이)
/// `impl TerminalHostWindow for MainWindow {}` 가 존재하지만 trait object 사용 0.
/// 도메인 계열 표현과 미래 StandaloneSurfaceWindow/StandaloneWorkspaceWindow
/// placeholder로 보존.
#[allow(dead_code)]
pub(crate) trait TerminalHostWindow: Window {
    /// 사이드바(워크스페이스 목록)를 가지는지. 기본 true.
    /// StandaloneSurfaceWindow 등은 override.
    fn has_sidebar(&self) -> bool {
        let engine = &self.engine_state;
        let _ = engine;
        true
    }
}

/// 터미널 호스트 윈도우 구현체가 `Window::modality`에 반환해야 하는 값.
/// MainWindow가 사용. `Window::modality()` trait dispatch가 호출 0이라
/// 추적상 dead로 잡히지만 도메인 표현으로 보존.
#[allow(dead_code)]
pub(crate) const MODELESS_MODALITY: Modality = Modality::Modeless;
