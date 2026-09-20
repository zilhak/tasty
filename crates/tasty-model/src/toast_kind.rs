//! Toast 의 위치 앵커 — 어느 영역에 떠오를지.
//!
//! 분류(`ToastKind`)는 색을 고르는 값이라 `tasty-type-appearance` 에 있고 여기서
//! 재수출한다 — 그려야 하는 쪽과 발화하는 쪽이 서로를 안 보고 같은 열거를 본다.
//! `ToastManager` / `ToastState` (UI 동작 본문) 는 본 바이너리 잔류.

/// 어느 영역에 떠오를지 결정하는 위치 앵커.
#[derive(Debug, Clone, PartialEq)]
pub enum ToastScope {
    Window,
    Workspace(usize),
    Pane(u32),
    Surface(u32),
}

pub use tasty_type_appearance::toast_kind::ToastKind;
