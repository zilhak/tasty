//! Window 트레잇 계층.
//!
//! ```text
//! Window (sealed trait)
//! ├── ModalWindow (supertrait)         — Settings, Quit, ...
//! └── TerminalHostWindow (supertrait)  — Main, StandaloneSurface, ...
//! ```
//!
//! 모든 구현체는 `WindowBase`를 composition하여 공통 필드를 공유한다.
//! `Window`는 sealed이므로 크레이트 외부에서 직접 구현할 수 없다.
//! 실제 구현체는 반드시 `ModalWindow` 또는 `TerminalHostWindow` 중 하나를 거쳐야 한다.

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
pub(crate) use modal::ModalWindow;
pub(crate) use plugins::PluginsWindow;
pub(crate) use preset::PresetWindow;
pub(crate) use quit::QuitWindow;
pub(crate) use settings::SettingsWindow;
pub(crate) use terminal_host::TerminalHostWindow;

/// `Box<dyn Window>`에서 `MainWindow` 소유권을 추출한다.
/// 인자가 MainWindow가 아니면 `None` — 호출자가 인지 후 다르게 처리.
pub(crate) fn unbox_main(w: Box<dyn Window>) -> Option<Box<MainWindow>> {
    if !w.as_any().is::<MainWindow>() {
        return None;
    }
    let any: Box<dyn std::any::Any> = w;
    any.downcast::<MainWindow>().ok()
}

use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;

use crate::AppEvent;

/// 윈도우의 모달리티.
///
/// 도메인 용어(`docs/design/ubiquitous-language.md`): Window는 modality
/// (Modeless/Modal)와 계열(ModalWindow/TerminalHostWindow)을 속성으로 갖는다.
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
pub(crate) enum WindowAction {
    /// 아무 일도 하지 않음.
    None,
    /// 이 윈도우를 닫음.
    Close,
    /// 이 윈도우를 닫고 AppEvent를 발행함.
    CloseWithEvent(AppEvent),
}

/// 이벤트 핸들러에 함께 전달되는 맥락.
pub(crate) struct WindowCtx<'a> {
    pub(crate) event_loop: &'a ActiveEventLoop,
    /// 현재 모달 윈도우가 활성 상태인지. true면 비모달 윈도우는 입력을 차단해야 한다.
    pub(crate) modal_active: bool,
    /// 현재 active plugin manager. 메인 윈도우가 frame prepare 시 plugin canvas의
    /// SharedMemory와 dirty rect에 접근하기 위해 사용한다. plugin 비활성 빌드/초기 시점에는 None.
    pub(crate) plugin_manager: Option<&'a crate::plugin::PluginManager>,
}

/// Sealed 모듈 — 외부에서 `Window`를 직접 구현하지 못하게 차단한다.
/// `ModalWindow` 또는 `TerminalHostWindow` 중 하나의 supertrait 체인을 경유해야 한다.
pub(crate) mod sealed {
    pub(crate) trait Sealed {}
}

/// 모든 윈도우 타입이 공유하는 최상위 트레잇.
///
/// 직접 구현하지 말고 `ModalWindow` 또는 `TerminalHostWindow` 중 하나를 구현하라.
/// 각 구현체는 `impl sealed::Sealed for MyWindow {}`를 별도로 추가해야 한다.
pub(crate) trait Window: sealed::Sealed + std::any::Any {
    fn base(&self) -> &WindowBase;
    fn base_mut(&mut self) -> &mut WindowBase;
    /// 도메인 표현 보존. trait dispatch 호출 0이지만 5개 구현체가 채워둠.
    /// modal 활성 판정 dispatch가 도입되면 활성화.
    #[allow(dead_code)]
    fn modality(&self) -> Modality;

    fn handle_event(&mut self, event: WindowEvent, ctx: &mut WindowCtx<'_>) -> WindowAction;
    fn render(&mut self);

    /// 모달 계열 다운캐스트. 모달이 아니면 `None`.
    /// 도메인 placeholder — `ModalWindow` 다운캐스트 dispatch가 도입되면 활성화.
    #[allow(dead_code)]
    fn as_modal(&self) -> Option<&dyn ModalWindow> {
        None
    }
    #[allow(dead_code)]
    fn as_modal_mut(&mut self) -> Option<&mut dyn ModalWindow> {
        None
    }

    /// MainWindow 다운캐스트. MainWindow가 아니면 `None`.
    fn as_main(&self) -> Option<&MainWindow> {
        self.as_any().downcast_ref::<MainWindow>()
    }
    fn as_main_mut(&mut self) -> Option<&mut MainWindow> {
        self.as_any_mut().downcast_mut::<MainWindow>()
    }

    /// `std::any::Any` 다운캐스트용.
    fn as_any(&self) -> &dyn std::any::Any;
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;

    fn mark_dirty(&mut self) {
        self.base_mut().dirty = true;
        self.base().winit.request_redraw();
    }
}
