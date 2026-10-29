//! The egui overlay backend of a WKWebView: popup holes in the layer mask or a sibling host view.
use objc2::DefinedClass;

use super::PlatformWebView;
use crate::state::webview_overlay::{OverlayMode, OverlayPaintSource};
use crate::webview::overlay::{OverlayApplyError, OverlayRequest, WebViewOverlayBackend};

/// 불투명 모드는 팝업 상자를 layer mask 구멍으로 빼 주 표면이 그린 팝업을 보인다(그림 위치 주 표면). 투명
/// 모드는 창 단위 오버레이 host(형제 NSView)에 그린다. 두 모드 모두 대상 팝업이 보이는 동안 뷰 전체 포인터를
/// host 로 보낸다. host 가 다시 보일 때 지난 그림을 지우지 못해(측정) 투명 모드는 보고하지 않는다.
/// 계약: docs/design/systems/webview.md 의 "egui 오버레이와 native WebView" 절.
impl WebViewOverlayBackend for PlatformWebView {
    fn overlay_support(&self) -> OverlayMode {
        OverlayMode::Opaque
    }

    fn overlay_paint_source(&self, mode: OverlayMode) -> OverlayPaintSource {
        if mode == OverlayMode::Opaque {
            OverlayPaintSource::MainSurface
        } else {
            OverlayPaintSource::OverlaySurface
        }
    }

    fn apply_overlay(&self, request: &OverlayRequest<'_>) -> Result<(), OverlayApplyError> {
        let overlay = if request.mode == OverlayMode::Opaque {
            request
                .regions
                .iter()
                .map(|r| [r.x, r.y, r.width, r.height].map(|v| v.value().round() as i32))
                .collect()
        } else {
            Vec::new()
        };
        {
            let mut mask = self.mask.borrow_mut();
            mask.overlay = overlay;
            mask.scale_factor = request.scale_factor;
        }
        self.webview
            .ivars()
            .host_pointer
            .set(request.host_takes_pointer);
        if self.apply_mask() {
            Ok(())
        } else {
            Err(OverlayApplyError::Failed(
                "the web view has no backing layer for the popup cutout".into(),
            ))
        }
    }

    fn clear_overlay(&self) {
        self.webview.ivars().host_pointer.set(false);
        self.mask.borrow_mut().overlay.clear();
        if !self.apply_mask() {
            tracing::warn!("webview: no backing layer while clearing the popup cutout");
        }
    }
}
