//! 토스트의 강조색을 결정하는 공용 종류.
//! 도메인과 그리기 코드가 egui 의존성 없이 같은 타입을 사용한다.

/// Toast 의 종류. 좌측 컬러 바 색을 결정.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Success,
    /// 경고 토스트.
    Warning,
    Error,
}

impl ToastKind {
    /// 모든 종류의 수동 목록. 새 종류를 추가할 때 이 배열도 함께 갱신해야 한다.
    /// 현재 목록의 완전성을 대조하는 검사는 없다.
    pub const ALL: &'static [ToastKind] = &[
        ToastKind::Info,
        ToastKind::Success,
        ToastKind::Warning,
        ToastKind::Error,
    ];
}
