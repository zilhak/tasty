//! Window 트레잇 *구현체* 모듈 + 보조 타입 (`Modality`, `ViewCtx`, `ViewAction`).
//!
//! D.3.E.3.c — `View` trait 본체 + `sealed` 모듈은 `src/view/ui.rs` 로 이동.
//! 구현체 모듈 (main / settings / quit / preset / plugins) 의 위치 이동은 .d.

pub mod base;
pub mod editor;
pub mod main;
pub mod modal;
pub mod plugins;
pub mod preset;
pub mod quit;
pub mod settings;
pub mod terminal_host;

pub(crate) use base::WindowBase;
pub(crate) use main::MainWindow;
pub(crate) use modal::ModalView;
pub(crate) use plugins::PluginsWindow;
pub(crate) use preset::PresetWindow;
pub(crate) use quit::QuitWindow;
pub(crate) use settings::SettingsWindow;
pub(crate) use terminal_host::TerminalHostView;

use crate::view::ui::View;

/// `Box<dyn View>`에서 `MainWindow` 소유권을 추출한다.
/// 인자가 MainWindow가 아니면 `None` — 호출자가 인지 후 다르게 처리.
pub(crate) fn unbox_main(w: Box<dyn View>) -> Option<Box<MainWindow>> {
    if !w.as_any().is::<MainWindow>() {
        return None;
    }
    let any: Box<dyn std::any::Any> = w;
    any.downcast::<MainWindow>().ok()
}

use winit::event_loop::ActiveEventLoop;

use crate::AppEvent;

/// 윈도우의 모달리티.
///
/// 도메인 용어(`docs/design/ubiquitous-language.md`): View는 modality
/// (Modeless/Modal)와 계열(ModalView/TerminalHostView/EditorView)을 속성으로 갖는다.
/// 현재 trait dispatch 경로에서 호출 0이지만 5개 구현체가 `fn modality()`로
/// 반환하는 도메인 표현이라 보존한다. modal 활성 판정 dispatch가 도입되면 활성화.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Modality {
    /// 일반 윈도우. 다른 윈도우와 독립적으로 포커스됨.
    Modeless,
    /// 모달 윈도우. 활성 상태에선 다른 모든 윈도우의 입력이 차단됨.
    /// 엔진 전역에서 최대 1개만 존재한다.
    Modal,
}

/// 이벤트 처리 결과로 윈도우가 요청하는 동작.
#[must_use]
pub(crate) enum ViewAction {
    /// 아무 일도 하지 않음.
    None,
    /// 이 윈도우를 닫음.
    Close,
    /// 이 윈도우를 닫고 AppEvent를 발행함.
    CloseWithEvent(AppEvent),
}

/// 이벤트 핸들러에 함께 전달되는 맥락.
pub(crate) struct ViewCtx<'a> {
    pub(crate) event_loop: &'a ActiveEventLoop,
    /// 현재 모달 윈도우가 활성 상태인지. true면 비모달 윈도우는 입력을 차단해야 한다.
    pub(crate) modal_active: bool,
    /// 현재 active plugin manager. 메인 윈도우가 frame prepare 시 plugin canvas의
    /// SharedMemory와 dirty rect에 접근하기 위해 사용한다. plugin 비활성 빌드/초기 시점에는 None.
    pub(crate) plugin_manager: Option<&'a crate::plugin::PluginManager>,
}
