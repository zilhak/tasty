//! Toast 표시 위치. 색상 분류는 tasty-type-appearance에서 재수출한다.
//! 실제 표시 상태와 관리는 호스트 UI가 맡는다.

/// 어느 영역에 떠오를지 결정하는 위치 앵커.
#[derive(Debug, Clone, PartialEq)]
pub enum ToastScope {
    Window,
    Workspace(usize),
    Pane(u32),
    Surface(u32),
}

pub use tasty_type_appearance::toast_kind::ToastKind;
