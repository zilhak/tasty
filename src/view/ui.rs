//! `View` trait — 엔진이 관리하는 render target 의 추상화.
//!
//! ```text
//! View (sealed trait)
//! ├── ModalView (supertrait)         — Settings, Quit, Plugins
//! ├── TerminalHostView (supertrait)  — Main, (StandaloneSurface 등 미래)
//! └── EditorView (supertrait)        — Preset, (키바인딩/테마 에디터 등 미래)
//! ```
//!
//! 모든 구현체는 `ViewBase` (D.3.E.3.e 에서 옛 `WindowBase` 에서 rename 완료) 를
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

use crate::view::{MainView, ModalView, Modality, ViewAction, ViewBase, ViewCtx};

/// 모든 View 타입이 공유하는 최상위 트레잇.
///
/// 직접 구현하지 말고 `ModalView` / `TerminalHostView` / `EditorView` 중
/// 하나를 구현하라. 각 구현체는 `impl sealed::Sealed for MyView {}` 를 별도로
/// 추가해야 한다.
pub(crate) trait View: sealed::Sealed + std::any::Any {
    fn base(&self) -> &ViewBase;
    fn base_mut(&mut self) -> &mut ViewBase;
    /// 도메인 표현 보존. trait dispatch 호출 0이지만 5개 구현체가 `fn modality()`로
    /// 반환하는 도메인 표현이라 보존한다. modal 활성 판정 dispatch가 도입되면 활성화.
    #[allow(dead_code)] // view 추상화 scaffolding — 5개 구현체 보유, dispatch 미배선
    fn modality(&self) -> Modality;

    fn handle_event(&mut self, event: WindowEvent, ctx: &mut ViewCtx<'_>) -> ViewAction;
    fn render(&mut self);

    /// 모달 계열 다운캐스트. 모달이 아니면 `None`.
    /// 도메인 placeholder — `ModalView` 다운캐스트 dispatch가 도입되면 활성화.
    #[allow(dead_code)] // view 추상화 scaffolding — 다운캐스트 dispatch 미배선
    fn as_modal(&self) -> Option<&dyn ModalView> {
        None
    }
    #[allow(dead_code)] // view 추상화 scaffolding — 다운캐스트 dispatch 미배선
    fn as_modal_mut(&mut self) -> Option<&mut dyn ModalView> {
        None
    }

    /// MainView 다운캐스트. MainView가 아니면 `None`.
    fn as_main(&self) -> Option<&MainView> {
        self.as_any().downcast_ref::<MainView>()
    }
    fn as_main_mut(&mut self) -> Option<&mut MainView> {
        self.as_any_mut().downcast_mut::<MainView>()
    }

    /// 배너(View 스코프) 표시 플레이스홀더 — 이 View 위에 View-스코프 배너가 뜰
    /// 위치. `screen` 은 현재 화면 rect. 기본값은 **콘텐츠 영역 최상단(탭바 아래)**
    /// 으로, 모든 View 가 별도 지정 없이도 합리적 위치를 갖는다(Modal 포함). 각 View
    /// 구현체는 자기에게 알맞은 곳으로 override 할 수 있다.
    ///
    /// 워크스페이스 전환과 무관하게 View 위에 유지된다(배너 발화 정책 §View 배너).
    /// `tab_bar_height` 만큼 내려 탭 바를 가리지 않는다.
    #[allow(dead_code)] // View 스코프 배너 플레이스홀더 — 호스트 루프 배선은 후속.
    fn banner_placeholder(&self, screen: egui::Rect) -> Option<egui::Rect> {
        let tab_bar = crate::theme::theme().tab_bar_height.value();
        Some(egui::Rect::from_min_max(
            egui::pos2(screen.left(), screen.top() + tab_bar),
            screen.max,
        ))
    }

    /// `std::any::Any` 다운캐스트용.
    fn as_any(&self) -> &dyn std::any::Any;
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;

    fn mark_dirty(&mut self) {
        self.base_mut().dirty = true;
        self.base().winit.request_redraw();
    }
}
