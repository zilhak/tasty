//! 방향 enum. 분할 / 포커스 이동의 방향 표현.

/// 분할 방향. `PhysicalRect::split` 에 전달.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SplitDirection {
    Horizontal,
    Vertical,
}

/// 포커스 이동 방향. UI 입력에서 *이웃 pane/surface 선택* 용도.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FocusDirection {
    Up,
    Down,
    Left,
    Right,
}
