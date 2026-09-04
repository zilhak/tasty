//! Toast 분류 enum — UI 본문이 아닌 *분류* 만 model 에 둔다.
//!
//! `ToastManager` / `ToastState` (UI 동작 본문) 는 [`crate::adapters::ui::toast`] 잔류.
//! headless 빌드에서도 intent 큐가 toast 를 발화할 수 있도록 GUI 의존 0.

/// Toast 의 종류. 좌측 컬러 바 색을 결정.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Success,
    /// 표준 toast kind — 향후 경고 발화 시 활성화.
    Warning,
    Error,
}

impl ToastKind {
    /// 모든 변종. 갤러리 미러(`tasty-gallery` 의 `catalog::toast_card::ToastKind`)와
    /// 이 집합이 갈리지 않는지 그쪽 테스트가 **런타임 열거**로 대조한다 — 새 변종을
    /// 여기 더하면 미러에도 더할 때까지 그 테스트가 실패한다. 변종을 추가할 때
    /// 이 배열을 함께 갱신하는 것이 그 대조의 전제다.
    pub const ALL: &'static [ToastKind] = &[
        ToastKind::Info,
        ToastKind::Success,
        ToastKind::Warning,
        ToastKind::Error,
    ];
}

/// 어느 영역에 떠오를지 결정하는 위치 앵커.
#[derive(Debug, Clone, PartialEq)]
pub enum ToastScope {
    Window,
    Workspace(usize),
    Pane(u32),
    Surface(u32),
}
