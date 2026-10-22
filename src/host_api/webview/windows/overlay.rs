//! Windows WebView 의 창 region 과 포인터 차단. 안내 카드 구멍과 대상 팝업 영역을 같은 창 region 에
//! 합치고(`window_region`), 대상 팝업이 보이는 동안 창을 꺼 포인터를 부모로 보낸다.

use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::{
    CombineRgn, CreateRectRgn, DeleteObject, HRGN, RGN_DIFF, RGN_ERROR, SetWindowRgn,
};

use super::super::overlay::{OverlayApplyError, OverlayRequest, WebViewOverlayBackend};
use super::super::window_region::RegionUpdate;
use super::PlatformWebView;
use crate::state::webview_overlay::{OverlayMode, OverlayPaintSource};

impl PlatformWebView {
    /// 보관한 구멍과 지금 HWND 크기로 창 region을 맞춘다. 바뀐 것이 없거나 적용에 성공하면 true다.
    pub(super) fn refresh_region(&self) -> bool {
        let update = self.region.borrow().pending(self.size.get());
        let applied = match &update {
            RegionUpdate::Unchanged => return true,
            // SAFETY: hwnd는 self가 살아 있는 동안 유효하다. main thread 호출. None은 region을 지운다.
            RegionUpdate::Clear => unsafe { SetWindowRgn(self.hwnd, None, true) != 0 },
            // SAFETY: 만든 region은 이 블록에서 지우거나, SetWindowRgn이 성공하면 시스템이 소유한다.
            // hwnd는 self가 살아 있는 동안 유효하다. main thread 호출.
            RegionUpdate::Set { size, holes } => unsafe {
                set_clip_region(self.hwnd, *size, holes)
            },
        };
        if applied {
            self.region.borrow_mut().mark_applied(&update);
        }
        applied
    }

    /// 이 창을 켜거나 끈다. 꺼진 창과 그 자식(WebView2 창)은 포인터를 받지 않아 그 자리의 클릭·휠이
    /// 부모로 간다. 끈 뒤 상태를 다시 읽어 실제로 바뀌었을 때만 true다.
    fn set_pointer_held(&self, held: bool) -> bool {
        use windows::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, IsWindowEnabled};
        // SAFETY: hwnd는 self가 살아 있는 동안 유효하다. main thread 호출. 반환값은 이전 상태라
        // 성공 여부로 읽지 않고 IsWindowEnabled로 확인한다.
        let enabled = unsafe {
            let _was_disabled = EnableWindow(self.hwnd, !held);
            IsWindowEnabled(self.hwnd).as_bool()
        };
        if enabled == held {
            return false;
        }
        self.pointer.borrow_mut().set(held);
        true
    }
}

/// 전체 사각형에서 구멍을 뺀 region을 `hwnd`에 설정한다. 성공하면 true다.
///
/// # Safety
/// `hwnd`는 유효한 창이어야 하고 그 창의 스레드에서 불러야 한다.
unsafe fn set_clip_region(hwnd: HWND, (w, h): (i32, i32), clip: &[[i32; 4]]) -> bool {
    // SAFETY: GDI region 생성·결합·삭제는 호출자 스레드에서 안전하다. 실패 시 각 handle을 지운다.
    unsafe {
        let region: HRGN = CreateRectRgn(0, 0, w, h);
        if region.is_invalid() {
            return false;
        }
        for &[x, y, cw, ch] in clip {
            let hole = CreateRectRgn(x, y, x + cw, y + ch);
            let combined = !hole.is_invalid()
                && CombineRgn(Some(region), Some(region), Some(hole), RGN_DIFF) != RGN_ERROR;
            if !hole.is_invalid() && !DeleteObject(hole.into()).as_bool() {
                tracing::warn!("DeleteObject failed for a WebView2 clip hole region");
            }
            if !combined {
                if !DeleteObject(region.into()).as_bool() {
                    tracing::warn!("DeleteObject failed for a WebView2 clip region");
                }
                return false;
            }
        }
        // 성공하면 region은 시스템 소유가 되어 지우면 안 된다. 실패하면 직접 지운다.
        if SetWindowRgn(hwnd, Some(region), true) == 0 {
            if !DeleteObject(region.into()).as_bool() {
                tracing::warn!("DeleteObject failed for a WebView2 clip region");
            }
            return false;
        }
        true
    }
}

/// 불투명 모드만 낸다. 팝업 영역을 창 region에서 빼 주 표면이 그린 팝업이 보이게 하고, 창을 꺼 WebView
/// 영역 전체의 포인터를 부모로 보낸다. 투명 합성 수단은 아직 측정하지 않았다.
/// 계약: docs/design/systems/webview.md 의 "egui 오버레이와 native WebView" 절.
impl WebViewOverlayBackend for PlatformWebView {
    fn overlay_support(&self) -> OverlayMode {
        OverlayMode::Hide
    }

    fn overlay_paint_source(&self, mode: OverlayMode) -> OverlayPaintSource {
        match mode {
            OverlayMode::Opaque => OverlayPaintSource::MainSurface,
            _ => OverlayPaintSource::OverlaySurface,
        }
    }

    fn apply_overlay(
        &self,
        request: &OverlayRequest<'_>,
    ) -> std::result::Result<(), OverlayApplyError> {
        if request.mode != OverlayMode::Opaque {
            return Err(OverlayApplyError::Unsupported);
        }
        // 영역은 이미 set_bounds와 같은 정수 픽셀 경계다. 반올림은 부동소수 오차만 없앤다.
        let holes: Vec<[i32; 4]> = request
            .regions
            .iter()
            .map(|r| [r.x, r.y, r.width, r.height].map(|v| v.value().round() as i32))
            .collect();
        self.region.borrow_mut().set_overlay(&holes);
        if !self.refresh_region() {
            return Err(OverlayApplyError::Failed(
                "SetWindowRgn did not take the popup region".into(),
            ));
        }
        let wanted = self.pointer.borrow().wants(request.host_takes_pointer);
        if let Some(held) = wanted
            && !self.set_pointer_held(held)
        {
            return Err(OverlayApplyError::Failed(format!(
                "EnableWindow did not {} the WebView2 window",
                if held { "disable" } else { "enable" }
            )));
        }
        Ok(())
    }

    fn clear_overlay(&self) {
        self.region.borrow_mut().clear_overlay();
        if !self.refresh_region() {
            tracing::warn!("WebView2 window region restore failed after a popup overlay");
        }
        let held = self.pointer.borrow().held();
        if held && !self.set_pointer_held(false) {
            tracing::warn!("EnableWindow did not enable the WebView2 window after a popup overlay");
        }
    }
}
