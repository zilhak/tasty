//! `View` trait — 엔진이 관리하는 render target 의 추상화.
//!
//! ```text
//! View (sealed trait)
//! ├── ModalView (supertrait)  — Settings, Quit, Plugins
//! ├── MainView                — 사이드바/워크스페이스 호스팅 (View + Sealed 직접 구현)
//! └── PresetView               — Modeless 에디터 (View + Sealed 직접 구현)
//! ```
//!
//! 모든 구현체는 `ViewBase` (D.3.E.3.e 에서 옛 `WindowBase` 에서 rename 완료) 를
//! composition 하여 공통 필드를 공유한다. `View` 는 sealed 이므로 크레이트
//! 외부에서 직접 구현할 수 없다 — 모달 계열은 `ModalView` supertrait 를 거치고,
//! 그 외(`MainView`/`PresetView`)는 `View` + `sealed::Sealed` 를 직접 구현한다
//! (trait object dispatch 실사용이 없던 `TerminalHostView`/`EditorView` marker
//! supertrait 및 `Modality`/`.modality()`/`.as_modal()` 스캐폴딩은 제거됨).
//!
//! D.3.E.3.c — 옛 `src/adapters/ui/window.rs::Window` trait 가 본 파일로 이동.
//! 구현체 모듈 (main/settings/quit/preset/plugins) 의 위치 이동은 D.3.E.3.d.

/// Sealed 모듈 — 외부에서 `View` 를 직접 구현하지 못하게 차단한다.
/// 모달 계열은 `ModalView` supertrait 를 경유하고, 그 외 구현체는 `View` +
/// `sealed::Sealed` 를 직접 구현한다.
pub(crate) mod sealed {
    pub(crate) trait Sealed {}
}

use winit::event::WindowEvent;

use crate::view::repaint::RepaintSource;
use crate::view::{MainView, ViewAction, ViewBase, ViewCtx};

/// 모든 View 타입이 공유하는 최상위 트레잇.
///
/// 모달 계열은 `ModalView` 를 구현하라. 그 외(`MainView`/`PresetView` 등)는 `View`
/// 를 직접 구현하면 된다. 각 구현체는 `impl sealed::Sealed for MyView {}` 를
/// 별도로 추가해야 한다.
pub(crate) trait View: sealed::Sealed + std::any::Any {
    fn base(&self) -> &ViewBase;
    fn base_mut(&mut self) -> &mut ViewBase;

    fn handle_event(&mut self, event: WindowEvent, ctx: &mut ViewCtx<'_>) -> ViewAction;
    fn render(&mut self);

    /// MainView 다운캐스트. MainView가 아니면 `None`.
    fn as_main(&self) -> Option<&MainView> {
        self.as_any().downcast_ref::<MainView>()
    }
    fn as_main_mut(&mut self) -> Option<&mut MainView> {
        self.as_any_mut().downcast_mut::<MainView>()
    }

    /// `std::any::Any` 다운캐스트용.
    fn as_any(&self) -> &dyn std::any::Any;
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;

    /// 사용자 조작발 리페인트 요청 — 상한 없이 즉시 발화한다.
    ///
    /// 출력·애니메이션 계열 유발원은 이 기본형이 아니라 [`Self::mark_dirty_from`] 으로
    /// 분류해야 상한이 걸린다.
    fn mark_dirty(&mut self) {
        self.mark_dirty_from(RepaintSource::Interactive);
    }

    /// 유발원을 밝힌 리페인트 요청. 상한 대상이면 `request_redraw()` 발화가 다음
    /// 프레임 창까지 미뤄진다 — `dirty` 는 그래도 즉시 세워지고, 미뤄진 발화는
    /// `about_to_wait` 이 `WaitUntil` 로 반드시 되살린다([`crate::view::repaint`]).
    fn mark_dirty_from(&mut self, source: RepaintSource) {
        let base = self.base_mut();
        base.dirty = true;
        if base
            .repaint
            .admit(source, std::time::Instant::now(), &base.winit)
        {
            base.winit.request_redraw();
        }
    }
}
