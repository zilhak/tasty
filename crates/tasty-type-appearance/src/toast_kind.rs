//! Toast 의 **분류** — 좌측 컬러 바 색을 결정하는 값.
//!
//! 색을 고르는 값이라 이 크레이트에 있고, 여기엔 egui 가 필수 의존이 아니라
//! headless 빌드도 이 타입을 그대로 쓴다. 그리는 쪽(`tasty-ui-widgets`)과
//! 발화하는 쪽(도메인 intent 큐)이 **같은 열거를 보되 서로를 안 보게** 하는 자리다.

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
