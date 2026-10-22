//! Linux WebView 의 오버레이 backend. 불투명은 팝업 상자를 X 창 shape 에서 빼 아래 주 표면의 그림을
//! 보이고, 투명은 페이지 그림을 그대로 두고 창 단위 오버레이 host(`overlay_host/linux.rs`)가 위에 그린다.
//! 두 모드 모두 포인터는 입력 shape 를 비워 host 로 돌린다. 투명을 낼 수 있는지는 host 가 합성 관리자를
//! 보고 정하고, 공통 동기화가 그 상한으로 낮춘다.
//! 계약: docs/design/systems/webview.md 의 "egui 오버레이와 native WebView" 절.

use super::super::linux_shape::OverlayCut;
use super::super::overlay::{OverlayApplyError, OverlayRequest, WebViewOverlayBackend};
use super::PlatformWebView;
use crate::state::webview_overlay::{OverlayMode, OverlayPaintSource};

impl WebViewOverlayBackend for PlatformWebView {
    fn overlay_support(&self) -> OverlayMode {
        if self.xfixes.is_some() {
            OverlayMode::Transparent
        } else {
            OverlayMode::Hide
        }
    }

    fn overlay_paint_source(&self, mode: OverlayMode) -> OverlayPaintSource {
        match mode {
            OverlayMode::Opaque => OverlayPaintSource::MainSurface,
            OverlayMode::Hide | OverlayMode::Transparent => OverlayPaintSource::OverlaySurface,
        }
    }

    fn apply_overlay(&self, request: &OverlayRequest<'_>) -> Result<(), OverlayApplyError> {
        self.assert_origin_thread();
        if request.mode == OverlayMode::Hide || self.xfixes.is_none() {
            return Err(OverlayApplyError::Unsupported);
        }
        self.shape
            .borrow_mut()
            .set_overlay(Some(OverlayCut::from_request(request)));
        self.refresh_shape();
        Ok(())
    }

    fn clear_overlay(&self) {
        self.assert_origin_thread();
        if self.shape.borrow_mut().set_overlay(None) {
            self.refresh_shape();
        }
    }
}
