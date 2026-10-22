//! Linux X11 창 단위 오버레이 host. 주 창마다 ARGB 최상위 관리 창 하나를 주 창의 transient 로 만든다.
//! 합성 관리자(`_NET_WM_CM_S<n>` 소유자)가 픽셀 알파를 합성한다. 계약은 상위 모듈 `overlay_host` 와
//! docs/design/systems/webview.md 의 "창 단위 오버레이 host"·"Linux 투명 구현" 절이다.
//!
//! - 창 관리자가 transient 를 주 창과 함께 최소화·작업 공간 이동·다른 앱과의 순서에 묶는다. override-redirect
//!   창은 이 셋을 따르지 않았다(측정). 이동·크기는 WM 이 옮기지 않으므로 이 host 가 따라간다.
//! - WM 이 위치 요청을 바꾸면(작업 영역 밖·전체화면 주 창) 그 주 창 위치에서는 합성을 낼 수 없다고 보고해
//!   다음 프레임부터 불투명으로 내린다([`fit`]).
//! - 포커스·활성화를 가져가지 않도록 입력 힌트 거짓, `_NET_WM_USER_TIME` 0, 작업 표시줄·페이저 제외,
//!   장식 없음, 빈 입력 shape 로 만든다. 전역 위(ABOVE) 상태는 두지 않는다.
//! - 자기 X 연결을 연다. 주 창 configure 와 합성 관리자 선택 변경을 이 연결의 이벤트로 받아, 왕복 요청은
//!   만들 때와 주 창이 바뀐 뒤 보이는 동안의 위치 조회에만 쓴다. 이 연결은 host 와 함께 닫힌다.

// 이유: Xlib 호출을 순서대로 묶는 FFI 모듈이다(같은 폴더 linux.rs 와 같다). 각 블록의 안전 조건은 한 연결·
// 한 창에 대한 것이라 호출마다 나누어도 조건이 같다.
#![allow(clippy::multiple_unsafe_ops_per_block)]

mod fit;

use std::cell::{Cell, RefCell};
use std::ffi::{CString, c_int, c_long, c_uint, c_ulong};
use std::ptr::NonNull;
use std::sync::Arc;

use winit::raw_window_handle::{
    HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle, XlibDisplayHandle,
    XlibWindowHandle,
};
use winit::window::Window;
use x11_dl::{xfixes, xlib};

use super::{HostComposite, OverlayHostError, OverlayNativeHost};
use fit::{Fit, Placement};

/// X SHAPE 확장의 입력 shape 종류(`ShapeInput`).
const SHAPE_INPUT: c_int = 2;
/// XFixes 선택 이벤트 마스크: 소유자 설정·소유 창 파괴·소유 클라이언트 종료.
const SELECTION_EVENTS: c_ulong = 1 | 2 | 4;
/// XFixes 선택 이벤트 중 새 소유자 설정(`XFixesSetSelectionOwnerNotify`).
const SET_SELECTION_OWNER: c_int = 0;

pub(super) fn create(window: &Arc<Window>) -> Result<Box<dyn OverlayNativeHost>, OverlayHostError> {
    let parent = match window.window_handle().map(|h| h.as_raw()) {
        Ok(RawWindowHandle::Xlib(h)) => h.window,
        _ => return Err(OverlayHostError::Unsupported),
    };
    let parent_display = match window.display_handle().map(|h| h.as_raw()) {
        Ok(RawDisplayHandle::Xlib(h)) => h.display,
        _ => return Err(OverlayHostError::Unsupported),
    };
    let xl = xlib::Xlib::open().map_err(|e| OverlayHostError::Failed(format!("Xlib: {e}")))?;
    let fixes = xfixes::Xlib::open().map_err(|_| OverlayHostError::Unsupported)?;
    // SAFETY: parent_display 는 winit 이 살아 있는 동안 유효한 Xlib 연결이고 window 가 그 창을 살려 둔다.
    // 이벤트 루프 스레드에서만 불린다(create_overlay_host 계약).
    let host = unsafe { LinuxHost::open(window.clone(), xl, fixes, parent, parent_display)? };
    Ok(Box::new(host))
}

struct LinuxHost {
    xl: xlib::Xlib,
    /// 이 host 가 연 X 연결. Drop 이 닫는다.
    display: *mut xlib::Display,
    screen: c_int,
    root: c_ulong,
    parent: c_ulong,
    overlay: c_ulong,
    colormap: c_ulong,
    /// 보일 때 창을 투명으로 지우는 GC(전경 0 = 알파 0).
    clear_gc: xlib::GC,
    fixes_event_base: c_int,
    cm_atom: c_ulong,
    /// 합성 관리자 선택에 소유자가 있다. 선택 이벤트로만 갱신한다.
    compositor: Cell<bool>,
    /// 주 창 위치와 오버레이 창 위치 요청·응답.
    fit: RefCell<Fit>,
    /// 마지막 위치 조회 뒤 주 창 configure 가 왔다.
    parent_dirty: Cell<bool>,
    shown: Cell<bool>,
    /// surface 와 이 host 보다 오래 부모 창을 살려 둔다(상위 계약). 필드 drop 은 Drop::drop 뒤다.
    _window: Arc<Window>,
}

impl LinuxHost {
    /// # Safety
    /// `parent_display` 는 유효한 Xlib 연결이거나 null 이고 `parent` 는 그 연결의 살아 있는 창이다.
    unsafe fn open(
        window: Arc<Window>,
        xl: xlib::Xlib,
        fixes: xfixes::Xlib,
        parent: c_ulong,
        parent_display: Option<NonNull<std::ffi::c_void>>,
    ) -> Result<Self, OverlayHostError> {
        // SAFETY: 호출자 계약. XDisplayString 은 연결이 살아 있는 동안 유효한 문자열을 돌려준다.
        let name = parent_display.map_or(std::ptr::null_mut(), |d| unsafe {
            (xl.XDisplayString)(d.as_ptr().cast())
        });
        // SAFETY: name 은 null 이거나 위 연결의 NUL 종료 문자열이다.
        let display = unsafe { (xl.XOpenDisplay)(name) };
        if display.is_null() {
            return Err(OverlayHostError::Failed(
                "cannot open an X connection".into(),
            ));
        }
        // SAFETY: display 는 방금 연 연결이다. 실패하면 아래 Probe 가 닫는다.
        let probe = unsafe { Probe::new(&xl, &fixes, display) };
        let probe = match probe {
            Ok(p) => p,
            Err(e) => {
                // SAFETY: display 는 이 함수가 연 연결이고 다른 곳에서 쓰지 않는다.
                unsafe { (xl.XCloseDisplay)(display) };
                return Err(e);
            }
        };
        // SAFETY: display·parent 는 유효하다(위·호출자 계약). 출력 인자는 지역 변수다.
        let placed = unsafe { query_placement(&xl, display, probe.root, parent) };
        // SAFETY: display 는 유효하고 visual 은 이 연결의 32bit TrueColor visual 이다.
        let (overlay, colormap, clear_gc) =
            unsafe { create_overlay_window(&xl, &fixes, display, &probe, parent, placed) };
        Ok(Self {
            xl,
            display,
            screen: probe.screen,
            root: probe.root,
            parent,
            overlay,
            colormap,
            clear_gc,
            fixes_event_base: probe.fixes_event_base,
            cm_atom: probe.cm_atom,
            compositor: Cell::new(true),
            fit: RefCell::new(Fit::new(placed)),
            parent_dirty: Cell::new(false),
            shown: Cell::new(false),
            _window: window,
        })
    }

    /// 쌓인 이벤트를 읽는다. 읽기만 하며 서버 왕복은 없다.
    fn drain(&self) {
        // SAFETY: display 는 self 가 살아 있는 동안 유효하고 이벤트 루프 스레드에서만 쓴다. XEvent 는
        // 모든 이벤트를 담는 union 이라 종류를 확인한 뒤 해당 구조체로 읽는다.
        unsafe {
            while (self.xl.XPending)(self.display) > 0 {
                let mut ev: xlib::XEvent = std::mem::zeroed();
                (self.xl.XNextEvent)(self.display, &mut ev);
                let kind = ev.get_type();
                if kind == xlib::ConfigureNotify && ev.configure.window == self.parent {
                    self.parent_dirty.set(true);
                } else if kind == xlib::ConfigureNotify && ev.configure.window == self.overlay {
                    // 오버레이 창은 root 의 자식이라(장식 없음, 재부모 없음) 실제·합성 이벤트 모두 화면 좌표다.
                    let c = ev.configure;
                    self.overlay_configured([c.x, c.y, c.width, c.height]);
                } else if kind == self.fixes_event_base {
                    let sel =
                        &*(&ev as *const xlib::XEvent).cast::<xfixes::XFixesSelectionNotifyEvent>();
                    if sel.selection == self.cm_atom {
                        self.compositor_changed(
                            sel.subtype == SET_SELECTION_OWNER && sel.owner != 0,
                        );
                    }
                }
            }
        }
    }

    fn compositor_changed(&self, present: bool) {
        if self.compositor.replace(present) == present {
            return;
        }
        if present {
            tracing::info!(
                "X compositing manager is back; WebView overlays may be transparent again"
            );
        } else {
            tracing::warn!(
                "X compositing manager went away; WebView overlays lower to opaque from the next frame"
            );
        }
    }

    fn overlay_configured(&self, actual: Placement) {
        if self.fit.borrow_mut().configured(actual) {
            tracing::warn!(
                ?actual,
                "the window manager moved the WebView overlay window off the main window; pages under popups use the opaque mode at this window position"
            );
        }
    }

    /// 주 창 configure 가 왔으면 위치를 다시 읽는다. 서버 왕복 두 번이다.
    fn read_parent_if_dirty(&self) {
        if !self.parent_dirty.replace(false) {
            return;
        }
        // SAFETY: display·root·parent 는 self 수명 동안 유효하다.
        let parent = unsafe { query_placement(&self.xl, self.display, self.root, self.parent) };
        self.fit.borrow_mut().parent_read(parent);
    }

    /// 주 창이 바뀌었으면 위치를 다시 읽어 오버레이 창을 맞춘다.
    fn place_if_dirty(&self) {
        self.read_parent_if_dirty();
        let Some([x, y, w, h]) = self.fit.borrow_mut().request() else {
            return;
        };
        // SAFETY: overlay 는 이 연결이 만든 살아 있는 창이다.
        unsafe {
            (self.xl.XMoveResizeWindow)(self.display, self.overlay, x, y, w as c_uint, h as c_uint);
            (self.xl.XFlush)(self.display);
        }
    }
}

impl OverlayNativeHost for LinuxHost {
    unsafe fn surface_target(&self) -> wgpu::SurfaceTargetUnsafe {
        let display = NonNull::new(self.display.cast()).expect("open() checked the connection");
        wgpu::SurfaceTargetUnsafe::RawHandle {
            raw_display_handle: RawDisplayHandle::Xlib(XlibDisplayHandle::new(
                Some(display),
                self.screen,
            )),
            raw_window_handle: RawWindowHandle::Xlib(XlibWindowHandle::new(self.overlay)),
        }
    }

    fn composite(&self) -> HostComposite {
        self.drain();
        // 거절된 뒤에는 주 창이 움직였는지 봐야 다시 시도할 수 있다. 숨긴 동안에도 이 판정이 필요하다.
        if self.fit.borrow().awaits_parent_move() {
            self.read_parent_if_dirty();
        }
        if self.compositor.get() && !self.fit.borrow().refused() {
            HostComposite::ExternalArgb
        } else {
            HostComposite::Unavailable
        }
    }

    fn follow(&self) {
        self.drain();
        // 숨긴 동안은 표시만 미룬다. 보일 때 한 번 맞춘다.
        if self.shown.get() {
            self.place_if_dirty();
        }
    }

    fn set_shown(&self, shown: bool) {
        self.shown.set(shown);
        if shown {
            self.drain();
            self.place_if_dirty();
        }
        // SAFETY: overlay·clear_gc 는 이 연결이 만든 살아 있는 자원이다.
        unsafe {
            if shown {
                (self.xl.XMapWindow)(self.display, self.overlay);
                // 배경이 None 이라 서버는 새 내용을 지우지 않는다. 첫 present 전에 지난 그림이나 아래
                // 화면 사본이 보이지 않도록 알파 0 으로 지운다.
                // 서버가 창 크기로 자른다.
                (self.xl.XFillRectangle)(
                    self.display,
                    self.overlay,
                    self.clear_gc,
                    0,
                    0,
                    0x7fff,
                    0x7fff,
                );
            } else {
                (self.xl.XUnmapWindow)(self.display, self.overlay);
            }
            (self.xl.XFlush)(self.display);
        }
    }

    /// WebView X 창은 주 창의 자식이라 이 최상위 창 위로 오지 못한다. 주 창과의 순서는 WM 의 transient
    /// 규칙이 지킨다. 여기서 XRaiseWindow 를 보내면 WM 이 주 창 무리를 다른 앱 위로 올릴 수 있어 보내지 않는다.
    fn raise(&self) {}
}

impl Drop for LinuxHost {
    fn drop(&mut self) {
        // SAFETY: 이 연결과 자원은 이 host 만 쓰며, 상위 계약에 따라 이 창으로 만든 surface 는 이미 drop 됐다.
        unsafe {
            (self.xl.XFreeGC)(self.display, self.clear_gc);
            (self.xl.XDestroyWindow)(self.display, self.overlay);
            (self.xl.XFreeColormap)(self.display, self.colormap);
            (self.xl.XCloseDisplay)(self.display);
        }
    }
}

/// 연결에서 읽은 만들기 조건.
struct Probe {
    screen: c_int,
    root: c_ulong,
    visual: xlib::XVisualInfo,
    fixes_event_base: c_int,
    cm_atom: c_ulong,
}

impl Probe {
    /// 합성 관리자·XFixes·SHAPE·32bit visual 을 확인하고 합성 관리자 선택 이벤트를 신청한다. 합성 관리자가
    /// 없으면 이 창에서는 투명을 내지 않는다(`Unsupported`).
    ///
    /// # Safety
    /// `display` 는 유효한 연결이다.
    unsafe fn new(
        xl: &xlib::Xlib,
        fixes: &xfixes::Xlib,
        display: *mut xlib::Display,
    ) -> Result<Self, OverlayHostError> {
        // SAFETY: 호출자 계약. 출력 인자는 지역 변수이고 이름은 NUL 종료 문자열이다.
        unsafe {
            let screen = (xl.XDefaultScreen)(display);
            let root = (xl.XRootWindow)(display, screen);
            let (mut event_base, mut error_base) = (0, 0);
            if (fixes.XFixesQueryExtension)(display, &mut event_base, &mut error_base) == 0 {
                return Err(OverlayHostError::Unsupported);
            }
            let (mut op, mut ev, mut err) = (0, 0, 0);
            if (xl.XQueryExtension)(display, c"SHAPE".as_ptr(), &mut op, &mut ev, &mut err) == 0 {
                return Err(OverlayHostError::Unsupported);
            }
            let cm_name = CString::new(format!("_NET_WM_CM_S{screen}")).expect("no NUL");
            let cm_atom = (xl.XInternAtom)(display, cm_name.as_ptr(), 0);
            // 신청을 먼저 해 소유자 확인과 신청 사이의 변경을 놓치지 않는다.
            (fixes.XFixesSelectSelectionInput)(display, root, cm_atom, SELECTION_EVENTS);
            if (xl.XGetSelectionOwner)(display, cm_atom) == 0 {
                return Err(OverlayHostError::Unsupported);
            }
            let mut visual: xlib::XVisualInfo = std::mem::zeroed();
            if (xl.XMatchVisualInfo)(display, screen, 32, xlib::TrueColor, &mut visual) == 0 {
                return Err(OverlayHostError::Unsupported);
            }
            Ok(Self {
                screen,
                root,
                visual,
                fixes_event_base: event_base,
                cm_atom,
            })
        }
    }
}

/// 주 창 클라이언트 영역의 화면 좌표. 서버 왕복 두 번이다.
///
/// # Safety
/// `display`·`root`·`parent` 가 유효하다.
unsafe fn query_placement(
    xl: &xlib::Xlib,
    display: *mut xlib::Display,
    root: c_ulong,
    parent: c_ulong,
) -> Placement {
    let (mut x, mut y, mut child) = (0, 0, 0);
    let (mut groot, mut gx, mut gy, mut w, mut h, mut border, mut depth) = (0, 0, 0, 1, 1, 0, 0);
    // SAFETY: 호출자 계약. 출력 인자는 지역 변수다.
    unsafe {
        (xl.XTranslateCoordinates)(display, parent, root, 0, 0, &mut x, &mut y, &mut child);
        (xl.XGetGeometry)(
            display,
            parent,
            &mut groot,
            &mut gx,
            &mut gy,
            &mut w,
            &mut h,
            &mut border,
            &mut depth,
        );
    }
    [x, y, w.max(1) as i32, h.max(1) as i32]
}

/// 오버레이 창을 만든다. 아직 보이지 않는다.
///
/// # Safety
/// `display`·`parent` 가 유효하고 `probe` 는 이 연결에서 읽었다.
unsafe fn create_overlay_window(
    xl: &xlib::Xlib,
    fixes: &xfixes::Xlib,
    display: *mut xlib::Display,
    probe: &Probe,
    parent: c_ulong,
    [x, y, w, h]: Placement,
) -> (c_ulong, c_ulong, xlib::GC) {
    // SAFETY: 호출자 계약. 속성·힌트 구조체는 지역 변수이거나 Xlib 이 할당해 여기서 해제한다.
    unsafe {
        let colormap =
            (xl.XCreateColormap)(display, probe.root, probe.visual.visual, xlib::AllocNone);
        let mut attrs: xlib::XSetWindowAttributes = std::mem::zeroed();
        // 배경 None: 크기 변경 때 서버가 검정으로 지우는 프레임이 없다(측정).
        attrs.background_pixmap = 0;
        attrs.border_pixel = 0;
        attrs.colormap = colormap;
        attrs.override_redirect = 0;
        let mask =
            xlib::CWBackPixmap | xlib::CWBorderPixel | xlib::CWColormap | xlib::CWOverrideRedirect;
        let overlay = (xl.XCreateWindow)(
            display,
            probe.root,
            x,
            y,
            w as c_uint,
            h as c_uint,
            0,
            32,
            xlib::InputOutput as c_uint,
            probe.visual.visual,
            mask,
            &mut attrs,
        );
        set_managed_hints(xl, display, overlay, parent, [x, y, w, h]);
        let empty = (fixes.XFixesCreateRegion)(display, std::ptr::null_mut(), 0);
        (fixes.XFixesSetWindowShapeRegion)(display, overlay, SHAPE_INPUT, 0, 0, empty);
        (fixes.XFixesDestroyRegion)(display, empty);
        (xl.XSelectInput)(display, parent, xlib::StructureNotifyMask);
        (xl.XSelectInput)(display, overlay, xlib::StructureNotifyMask);
        let clear_gc = (xl.XCreateGC)(display, overlay, 0, std::ptr::null_mut());
        (xl.XSetForeground)(display, clear_gc, 0);
        // 만들기 요청을 서버가 처리하게 한다. 만들 때 한 번이다.
        (xl.XSync)(display, 0);
        (overlay, colormap, clear_gc)
    }
}

/// 관리 창 힌트. 주 창의 transient, 입력·포커스 요청 없음, 작업 표시줄·페이저 제외, 장식 없음, utility 종류.
///
/// # Safety
/// `display`·`overlay`·`parent` 가 유효하다.
unsafe fn set_managed_hints(
    xl: &xlib::Xlib,
    display: *mut xlib::Display,
    overlay: c_ulong,
    parent: c_ulong,
    [x, y, w, h]: Placement,
) {
    // SAFETY: 호출자 계약. 이름은 NUL 종료 문자열이고 속성 값은 지역 배열이다.
    unsafe {
        let atom = |name: &std::ffi::CStr| (xl.XInternAtom)(display, name.as_ptr(), 0);
        let set_longs = |property: c_ulong, kind: c_ulong, values: &[c_long]| {
            (xl.XChangeProperty)(
                display,
                overlay,
                property,
                kind,
                32,
                xlib::PropModeReplace,
                values.as_ptr().cast(),
                values.len() as c_int,
            );
        };
        (xl.XSetTransientForHint)(display, overlay, parent);
        let wm = (xl.XAllocWMHints)();
        if !wm.is_null() {
            (*wm).flags = xlib::InputHint;
            (*wm).input = 0;
            (xl.XSetWMHints)(display, overlay, wm);
            (xl.XFree)(wm.cast());
        }
        let size = (xl.XAllocSizeHints)();
        if !size.is_null() {
            (*size).flags = xlib::USPosition | xlib::PPosition | xlib::USSize | xlib::PSize;
            ((*size).x, (*size).y, (*size).width, (*size).height) = (x, y, w, h);
            (xl.XSetWMNormalHints)(display, overlay, size);
            (xl.XFree)(size.cast());
        }
        set_longs(atom(c"_NET_WM_USER_TIME"), xlib::XA_CARDINAL, &[0]);
        let state = [
            atom(c"_NET_WM_STATE_SKIP_TASKBAR") as c_long,
            atom(c"_NET_WM_STATE_SKIP_PAGER") as c_long,
        ];
        set_longs(atom(c"_NET_WM_STATE"), xlib::XA_ATOM, &state);
        let motif = atom(c"_MOTIF_WM_HINTS");
        // flags = MWM_HINTS_DECORATIONS, decorations = 0.
        set_longs(motif, motif, &[2, 0, 0, 0, 0]);
        let utility = atom(c"_NET_WM_WINDOW_TYPE_UTILITY") as c_long;
        set_longs(atom(c"_NET_WM_WINDOW_TYPE"), xlib::XA_ATOM, &[utility]);
    }
}
