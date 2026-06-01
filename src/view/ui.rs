//! `View` trait — 엔진이 관리하는 render target 의 추상화.
//!
//! ```text
//! View (sealed trait)
//! ├── ModalView (supertrait)         — Settings, Quit, Plugins
//! ├── TerminalHostView (supertrait)  — Main, (StandaloneSurface 등 미래)
//! └── EditorView (supertrait)        — Preset, (키바인딩/테마 에디터 등 미래)
//! ```
//!
//! 모든 구현체는 `WindowBase` (D.3.E.3.e 에서 `ViewBase` rename 예정) 를
//! composition 하여 공통 필드를 공유한다. `View` 는 sealed 이므로 크레이트
//! 외부에서 직접 구현할 수 없다 — 실제 구현체는 반드시 `ModalView` /
//! `TerminalHostView` / `EditorView` 중 하나의 supertrait 체인을 거쳐야 한다.
//!
//! D.3.E.3.c — 옛 `src/adapters/ui/window.rs::Window` trait 가 본 파일로 이동.
//! 구현체 모듈 (main/settings/quit/preset/plugins) 의 위치 이동은 D.3.E.3.d.

/// Sealed 모듈 — 외부에서 `View` 를 직접 구현하지 못하게 차단한다.
/// `ModalView` / `TerminalHostView` / `EditorView` 중 하나의 supertrait 체인을 경유해야 한다.
pub(crate) mod sealed {
    pub(crate) trait Sealed {}
}

use winit::event::WindowEvent;

use crate::view::{MainWindow, ModalView, Modality, ViewAction, ViewCtx, WindowBase};

/// 모든 윈도우 타입이 공유하는 최상위 트레잇.
///
/// 직접 구현하지 말고 `ModalView` / `TerminalHostView` / `EditorView` 중
/// 하나를 구현하라. 각 구현체는 `impl sealed::Sealed for MyWindow {}` 를 별도로
/// 추가해야 한다.
pub(crate) trait View: sealed::Sealed + std::any::Any {
    fn base(&self) -> &WindowBase;
    fn base_mut(&mut self) -> &mut WindowBase;
    /// 도메인 표현 보존. trait dispatch 호출 0이지만 5개 구현체가 `fn modality()`로
    /// 반환하는 도메인 표현이라 보존한다. modal 활성 판정 dispatch가 도입되면 활성화.
    #[allow(dead_code)]
    fn modality(&self) -> Modality;

    fn handle_event(&mut self, event: WindowEvent, ctx: &mut ViewCtx<'_>) -> ViewAction;
    fn render(&mut self);

    /// 모달 계열 다운캐스트. 모달이 아니면 `None`.
    /// 도메인 placeholder — `ModalView` 다운캐스트 dispatch가 도입되면 활성화.
    #[allow(dead_code)]
    fn as_modal(&self) -> Option<&dyn ModalView> {
        None
    }
    #[allow(dead_code)]
    fn as_modal_mut(&mut self) -> Option<&mut dyn ModalView> {
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
