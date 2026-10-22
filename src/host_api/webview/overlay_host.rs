//! 창 단위 native 오버레이 host. 페이지 단위 계약([`super::overlay::WebViewOverlayBackend`])과 나뉜다.
//! 주 창 하나에 별도 native 표면 하나를 붙여 그림 위치가 오버레이 창인 모드의 팝업을 그린다.
//! 계약은 docs/design/systems/webview.md 의 "창 단위 오버레이 host" 절이다.
//!
//! OS 구현은 `overlay_host/{linux,macos,windows}.rs` 의 `create` 다. 구현하지 않은 OS 는
//! [`OverlayHostError::Unsupported`] 를 돌려주고, host 는 그 창에서 다시 만들지 않는다.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

use std::sync::Arc;

use winit::window::Window;

/// 주 창 하나에 붙는 별도 native 오버레이 표면. 주 창 클라이언트 영역과 원점·물리 크기가 같고 포커스·창
/// 활성화·포인터를 받지 않는다. 이벤트 루프 스레드에서만 만들고 쓰고 버린다(`Send` 가 아니다).
///
/// 구현은 부모 창의 `Arc<Window>` 를 필드로 보유한다. 이 표면으로 만든 wgpu surface 는 host 보다 먼저
/// drop 되므로(`OverlaySurfaceParts` 필드 순서) host 가 보유한 창이 surface 수명 내내 부모 창과 그
/// display 연결을 살려 둔다. 숨김 상태에서는 그림이 보이지 않아야 하고, 숨긴 동안에도 surface 에 present
/// 할 수 있어야 한다. 보인 직후에는 지난 구간 그림이 없어야 한다. 마지막 준비 그림 또는 완전히 투명한 빈
/// 그림을 허용하며, 페이지는 보이는 host 에 이번 그림을 present 한 뒤에만 드러낸다. 공통 훅은 이번 구간
/// 그림을 숨긴 채 present 한 뒤에만 보인다. present 반환은 화면 반영 확인이 아니므로 보인 직후 첫 화면은
/// OS 마다 측정한다.
pub(crate) trait OverlayNativeHost {
    /// wgpu surface 대상. 반환한 핸들은 self 가 drop 될 때까지 바뀌지 않고 유효하다.
    ///
    /// # Safety
    /// 호출자는 이 대상으로 만든 surface 를 self 보다 먼저 drop 한다.
    unsafe fn surface_target(&self) -> wgpu::SurfaceTargetUnsafe;
    /// 이 host 가 지금 낼 수 있는 합성. 대상 팝업이 떠 있는 프레임에만 묻는다. 왕복 요청이 필요한 판정은
    /// host 가 캐시한다.
    fn composite(&self) -> HostComposite;
    /// 보유한 부모 창의 현재 클라이언트 위치·크기에 맞춘다. 같은 값이면 OS 를 부르지 않는다.
    fn follow(&self);
    /// 보이기·숨기기. 바뀔 때만 불린다. 보이기는 숨긴 채 이번 구간 그림을 present 한 뒤에만 불린다.
    fn set_shown(&self, shown: bool);
    /// 이 창의 WebView native 창들보다 위로 올린다.
    fn raise(&self);
}

/// host 의 합성 계약. wgpu surface 의 alpha 설정과 별개다. 투명 합성은 그 OS·조건에서 측정으로 확인한
/// host 만 보고한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
// 이유: macOS는 SwapchainAlpha, Linux는 ExternalArgb·Unavailable을 보고한다. OpaqueOnly는 시험용 host가 만든다.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) enum HostComposite {
    /// 불투명 사각형만 낸다. 알파는 무시된다.
    OpaqueOnly,
    /// 외부 합성기가 ARGB 픽셀 알파를 반영한다. swapchain alpha 가 `Opaque` 여도 된다.
    ExternalArgb,
    /// swapchain alpha(`PreMultiplied`·`PostMultiplied`)가 있어야 알파가 반영된다.
    SwapchainAlpha,
    /// 지금 조건에서 합성할 수 없다(합성 관리자 소실 등).
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OverlayHostError {
    /// 이 OS·세션은 창 단위 host 를 만들지 않는다. 그 창에서 다시 시도하지 않는다.
    Unsupported,
    /// 만들거나 붙이지 못했다. 대상 팝업 구간이 끝나면 한 번 다시 시도한다.
    Failed(String),
}

impl std::fmt::Display for OverlayHostError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported => f.write_str("overlay host not supported here"),
            Self::Failed(m) => write!(f, "overlay host failed: {m}"),
        }
    }
}

/// 이 OS 의 host 를 만든다. 부모 창의 `Arc` 를 넘겨 host 가 보유하게 한다.
pub(crate) fn create_overlay_host(
    window: &Arc<Window>,
) -> Result<Box<dyn OverlayNativeHost>, OverlayHostError> {
    #[cfg(target_os = "linux")]
    return linux::create(window);
    #[cfg(target_os = "macos")]
    return macos::create(window);
    #[cfg(windows)]
    return windows::create(window);
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        let _window = window;
        Err(OverlayHostError::Unsupported)
    }
}
