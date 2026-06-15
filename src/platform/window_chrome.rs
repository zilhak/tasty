//! CSD(Client-Side Decorations) 윈도우 속성 — OS별 데코레이션 전략 적용.
//!
//! 원칙 1(사용자/에이전트 분리)·원칙 4(크로스플랫폼). macOS 는 fullsize-content-view
//! 패턴으로 **네이티브 신호등을 유지**하면서 콘텐츠를 타이틀바 영역(y=0)까지 확장한다.
//! `with_decorations(false)` 는 신호등까지 없애므로 (a) 결정에서 쓰지 않는다.
//! Windows/Linux 의 CSD 전환(캡션 버튼/리사이즈)은 P5/P6 후속이라 현재는 no-op.

use winit::window::WindowAttributes;

/// 윈도우 생성부(첫 윈도우 + 추가 윈도우 공통)에서 호출해 OS별 CSD 속성을 적용한다.
///
/// - **macOS**: `titlebar_transparent` + `fullsize_content_view` + `title_hidden` 조합.
///   타이틀바를 투명화하고 콘텐츠를 y=0 까지 확장하되 OS 신호등(standardWindowButton:
///   close/min/zoom)은 그대로 둔다. 신호등의 클릭동작·hover글리프·풀스크린·접근성·
///   다크모드 디밍은 모두 OS 가 처리한다.
/// - **그 외 OS**: 변경 없음(네이티브 데코 유지).
pub fn apply_csd_attributes(attrs: WindowAttributes) -> WindowAttributes {
    #[cfg(target_os = "macos")]
    {
        use winit::platform::macos::WindowAttributesExtMacOS;
        attrs
            .with_titlebar_transparent(true)
            .with_fullsize_content_view(true)
            .with_title_hidden(true)
    }
    #[cfg(not(target_os = "macos"))]
    {
        attrs
    }
}
