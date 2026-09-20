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
    /// 모든 변종.
    ///
    /// **이 배열을 읽는 곳은 지금 하나도 없다** — 갤러리가 이 열거의 미러를 들고 있던
    /// 동안 두 `ALL` 을 런타임에 열거해 대조하는 시험이 유일한 소비자였고, 갤러리가
    /// 정본을 직접 받게 되면서 미러와 그 시험이 함께 내려갔다. 지금 변종이 갈리는 것은
    /// `match` 비-소진으로 **컴파일이** 잡는다([ADR-0329](../../../docs/adr/0329-a-mirror-is-removed-before-it-is-compared.md)).
    ///
    /// 그래서 이 배열이 최신인지를 재는 채널도 없다 — 변종을 더하고 여기를 안 고쳐도
    /// 아무 데서도 안 울린다. 새로 소비자를 붙일 때 그 사실을 먼저 보고 붙여라.
    pub const ALL: &'static [ToastKind] = &[
        ToastKind::Info,
        ToastKind::Success,
        ToastKind::Warning,
        ToastKind::Error,
    ];
}
