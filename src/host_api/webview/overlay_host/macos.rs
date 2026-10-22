//! macOS 창 단위 오버레이 host. winit 뷰 안에서 WKWebView 들의 형제로 맨 위에 붙는 투명 NSView 다.
//! wgpu 가 이 뷰에 Metal layer 를 만들고 같은 창의 Core Animation layer 트리 안에서 합성한다. 새 창을
//! 만들지 않으므로 창 이동·최소화·Space 는 주 창과 함께 간다. 포인터는 받지 않고(`hitTest:` nil) first
//! responder 가 되지 않는다. 계약은 상위 모듈 `overlay_host` 와 docs/design/systems/webview.md 를 따른다.

use std::ptr::NonNull;
use std::sync::Arc;

use objc2::rc::Retained;
use objc2::runtime::NSObjectProtocol;
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{NSAutoresizingMaskOptions, NSView, NSWindowOrderingMode};
use objc2_foundation::{NSPoint, NSRect};
use winit::raw_window_handle::{
    AppKitDisplayHandle, AppKitWindowHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle,
};
use winit::window::Window;

use super::{HostComposite, OverlayHostError, OverlayNativeHost};

define_class!(
    // SAFETY:
    // - NSView 는 서브클래싱을 허용한다. 지정 초기화자 `initWithFrame:` 를 아래 `create` 가 부른다.
    // - `MainThreadOnly` 가 맞다: NSView 는 main thread 전용이고 아래 오버라이드는 AppKit 이 main
    //   thread 에서 부른다.
    // - `Drop` 을 구현하지 않는다.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "TastyOverlayView"]
    struct OverlayView;

    unsafe impl NSObjectProtocol for OverlayView {}

    impl OverlayView {
        /// 포인터는 언제나 아래 뷰(WKWebView 또는 winit 뷰)로 간다.
        #[unsafe(method(hitTest:))]
        fn hit_test(&self, _point: NSPoint) -> *mut NSView {
            std::ptr::null_mut()
        }

        /// first responder 가 되지 않는다. define_class 가 Bool 반환 shim 을 만들므로 마지막 식으로 돌려준다.
        #[unsafe(method(acceptsFirstResponder))]
        fn accepts_first_responder(&self) -> bool {
            false
        }
    }
);

struct MacOverlayHost {
    view: Retained<OverlayView>,
    /// 부모 창. wgpu surface 가 이 host 보다 먼저 drop 되는 동안 winit 뷰와 NSWindow 를 살려 둔다.
    _window: Arc<Window>,
}

pub(super) fn create(window: &Arc<Window>) -> Result<Box<dyn OverlayNativeHost>, OverlayHostError> {
    let mtm = MainThreadMarker::new()
        .ok_or_else(|| OverlayHostError::Failed("overlay host needs the main thread".into()))?;
    let handle = window
        .window_handle()
        .map_err(|e| OverlayHostError::Failed(format!("window handle: {e}")))?;
    let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
        return Err(OverlayHostError::Unsupported);
    };
    // SAFETY: winit 이 만든 활성 NSView 다. 아래에서 window Arc 를 보유하므로 host 수명 동안 산다. main
    // thread 는 위 mtm 으로 확인했다.
    let parent: &NSView = unsafe { appkit.ns_view.cast::<NSView>().as_ref() };
    let view: Retained<OverlayView> = {
        let this = OverlayView::alloc(mtm);
        // SAFETY: NSView 의 지정 초기화자다.
        unsafe { msg_send![this, initWithFrame: parent.bounds()] }
    };
    view.setAutoresizingMask(
        NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
    );
    view.setWantsLayer(true);
    view.setHidden(true);
    parent.addSubview_positioned_relativeTo(&view, NSWindowOrderingMode::Above, None);
    Ok(Box::new(MacOverlayHost {
        view,
        _window: window.clone(),
    }))
}

impl OverlayNativeHost for MacOverlayHost {
    unsafe fn surface_target(&self) -> wgpu::SurfaceTargetUnsafe {
        let ns_view = NonNull::from(&*self.view).cast();
        wgpu::SurfaceTargetUnsafe::RawHandle {
            raw_display_handle: RawDisplayHandle::AppKit(AppKitDisplayHandle::new()),
            raw_window_handle: RawWindowHandle::AppKit(AppKitWindowHandle::new(ns_view)),
        }
    }

    /// 같은 창 layer 트리 안의 합성이라 swapchain alpha(`PostMultiplied` 의 `opaque = false`)가 있어야 알파가
    /// 반영된다. 이 값은 수단을 말할 뿐이며 투명 지원 보고는 WebView backend 가 측정 뒤에 정한다.
    fn composite(&self) -> HostComposite {
        HostComposite::SwapchainAlpha
    }

    fn follow(&self) {
        // SAFETY: main thread AppKit. 부모는 보유한 window 가 살려 둔다.
        let Some(parent) = (unsafe { self.view.superview() }) else {
            return;
        };
        let bounds: NSRect = parent.bounds();
        if self.view.frame() != bounds {
            self.view.setFrame(bounds);
        }
    }

    /// Metal layer 는 마지막 present 를 들고 있어 숨겼다 다시 보이면 첫 present 전까지 지난 그림이 보인다(측정).
    /// contents 를 비우면 그 뒤 present 도 보이지 않아(측정) 이 host 만으로는 지우지 못한다. 그래서 이 backend 는
    /// 투명 모드를 보고하지 않는다.
    fn set_shown(&self, shown: bool) {
        self.view.setHidden(!shown);
    }

    /// 새 WKWebView 는 `addSubview:` 로 형제 맨 위에 들어오므로 그 뒤 다시 맨 위로 올린다. 이미 맨 위면
    /// 계층을 바꾸지 않는다.
    fn raise(&self) {
        // SAFETY: main thread AppKit. 부모는 보유한 window 가 살려 둔다.
        let Some(parent) = (unsafe { self.view.superview() }) else {
            return;
        };
        let top = parent.subviews().lastObject();
        let me: &NSView = &self.view;
        let on_top = top.is_some_and(|v| std::ptr::eq(&*v, me));
        if !on_top {
            parent.addSubview_positioned_relativeTo(&self.view, NSWindowOrderingMode::Above, None);
        }
    }
}

impl Drop for MacOverlayHost {
    fn drop(&mut self) {
        self.view.removeFromSuperview();
    }
}
