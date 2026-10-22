//! native WebView 위에 egui 대상 팝업을 합성하는 backend 계약. 계약 전체와 OS 별 담당 범위는
//! docs/design/systems/webview.md 의 "egui 오버레이와 native WebView" 절에 있다.
//!
//! 기본 구현은 "지원 안 함"이다. 지원 모드는 `Hide` 이고 적용은 `Unsupported` 로 실패한다. 이
//! 기본값을 쓰는 OS 는 대상 팝업이 뜨면 지금처럼 모든 WebView 를 숨긴다. 플랫폼 backend 는 실제로
//! 낼 수 있는 모드만 보고해야 하며, 보고한 모드의 적용이 실패하면 host 가 그 프레임부터 `Hide` 로
//! 돌아간다.

use crate::model::PhysicalRect;
use crate::state::webview_overlay::{OverlayMode, OverlayPaintSource};

/// 대상 팝업이 떠 있는 프레임마다 WebView 하나에 보내는 요청.
// 이유: 시험이 아닌 빌드에서 모든 필드를 읽는 backend 가 없다. 기본 구현을 쓰는 OS 는 요청을 읽지 않고,
// Linux backend 는 물리 px 영역만 쓰므로 `scale_factor` 를 읽지 않는다. 시험용 backend 가 모두 읽는다.
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone)]
pub struct OverlayRequest<'a> {
    /// `Opaque` 또는 `Transparent`. `Hide` 는 요청하지 않고 [`WebViewOverlayBackend::clear_overlay`] 를 부른다.
    pub mode: OverlayMode,
    /// host 가 칠할 영역. WebView 창(반올림한 `set_bounds` 사각형) 기준 물리 정수 픽셀이다.
    /// 불투명 모드는 팝업·드롭다운 상자와 바깥 1px 테두리, 투명 모드는 그림자·scrim 범위까지 들어 있다.
    pub regions: &'a [PhysicalRect],
    /// 참이면 WebView 영역 전체의 포인터(클릭·휠)를 host 가 받는다. 페이지에는 가지 않는다.
    pub host_takes_pointer: bool,
    pub scale_factor: f64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OverlayApplyError {
    /// 이 backend 는 요청한 모드를 낼 수 없다.
    Unsupported,
    /// 지원하지만 이번 적용이 실패했다(창 생성·shape·surface 실패 등).
    // 이유: 이 실패를 내는 플랫폼 backend 가 아직 없다. 시험용 backend 가 만든다.
    #[cfg_attr(not(test), allow(dead_code))]
    Failed(String),
}

impl std::fmt::Display for OverlayApplyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported => f.write_str("overlay mode not supported by this backend"),
            Self::Failed(m) => write!(f, "overlay apply failed: {m}"),
        }
    }
}

/// native WebView backend 가 오버레이 합성을 위해 제공하는 메서드. OS 별 `PlatformWebView` 가
/// 구현하며, 구현하지 않은 메서드는 "지원 안 함" 동작이다.
pub trait WebViewOverlayBackend {
    /// 이 WebView 가 지금 낼 수 있는 가장 좋은 모드. 매 프레임 묻는다(합성 관리자 변화 등).
    fn overlay_support(&self) -> OverlayMode {
        OverlayMode::Hide
    }

    /// 지원하는 `mode` 의 팝업 그림을 어디에 올리는지. 페이지 대상 영역에 구멍을 내 주 표면을 보이는
    /// backend 는 [`OverlayPaintSource::MainSurface`] 를 돌려준다. 기본값은 별도 오버레이 창이며, 그 창이
    /// 그림을 present 해야 페이지를 드러낸다.
    fn overlay_paint_source(&self, _mode: OverlayMode) -> OverlayPaintSource {
        OverlayPaintSource::OverlaySurface
    }

    /// 대상 팝업이 떠 있는 프레임마다 부른다. 실패하면 host 는 모든 WebView 를 숨긴다.
    fn apply_overlay(&self, _request: &OverlayRequest<'_>) -> Result<(), OverlayApplyError> {
        Err(OverlayApplyError::Unsupported)
    }

    /// 대상 팝업이 모두 닫혔거나 `Hide` 로 돌아갈 때 부른다. 페이지 입력과 그림을 원래대로 돌린다.
    /// `set_native_holes` 의 분할선·안내 카드 구멍은 이 호출과 별개로 유지된다.
    fn clear_overlay(&self) {}
}

#[cfg(test)]
pub(crate) mod fake {
    use std::cell::RefCell;

    use super::*;

    /// 받은 요청 하나: 모드, WebView 기준 영역(x, y, w, h), 호스트 포인터 여부.
    pub(crate) type AppliedRequest = (OverlayMode, Vec<[i32; 4]>, bool);

    /// 시험용 backend. 보고할 지원 모드·그림 위치와 적용 결과를 정하고 받은 요청과 표시 상태를 기록한다.
    #[derive(Default)]
    pub(crate) struct FakeOverlayBackend {
        pub support: OverlayMode,
        pub paint_source: OverlayPaintSource,
        /// 참이면 `Opaque` 만 주 표면 구멍으로 그린다(나머지 모드는 `paint_source`).
        pub opaque_on_main: bool,
        /// host 가 마지막으로 정한 페이지 표시 여부.
        pub visible: std::cell::Cell<bool>,
        pub fail: bool,
        pub applied: RefCell<Vec<AppliedRequest>>,
        pub scale_factors: RefCell<Vec<f64>>,
        pub cleared: RefCell<usize>,
        /// 마지막으로 성공한 적용이 host 포인터를 가져갔고 아직 `clear_overlay` 로 풀지 않았다.
        pub pointer_held: RefCell<bool>,
    }

    impl WebViewOverlayBackend for FakeOverlayBackend {
        fn overlay_support(&self) -> OverlayMode {
            self.support
        }

        fn overlay_paint_source(&self, mode: OverlayMode) -> OverlayPaintSource {
            if self.opaque_on_main && mode == OverlayMode::Opaque {
                OverlayPaintSource::MainSurface
            } else {
                self.paint_source
            }
        }

        fn apply_overlay(&self, request: &OverlayRequest<'_>) -> Result<(), OverlayApplyError> {
            if self.fail {
                return Err(OverlayApplyError::Failed("fake".into()));
            }
            self.scale_factors.borrow_mut().push(request.scale_factor);
            self.applied.borrow_mut().push((
                request.mode,
                request
                    .regions
                    .iter()
                    .map(|r| [r.x, r.y, r.width, r.height].map(|v| v.value() as i32))
                    .collect(),
                request.host_takes_pointer,
            ));
            *self.pointer_held.borrow_mut() = request.host_takes_pointer;
            Ok(())
        }

        fn clear_overlay(&self) {
            *self.cleared.borrow_mut() += 1;
            *self.pointer_held.borrow_mut() = false;
        }
    }
}
