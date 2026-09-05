//! Linux native context menu using GTK 3 Menu + popup_at_rect (X11 only).
//!
//! GTK is initialized lazily on first call. Unlike the macOS / Windows
//! backends — which track the popup inside the run loop / message pump the
//! main window already owns and therefore answer synchronously — GTK needs an
//! event loop of *its own* iterated to drive the menu. Spinning that loop
//! inline would block winit's event loop for as long as the menu is up (no
//! `_NET_WM_PING` reply → the WM paints the app "not responding", no input,
//! no rendering), so this backend **returns immediately** with
//! `MenuOutcome::Pending` and hands back a [`GtkMenuHandle`] the caller pumps
//! once per frame until it reports a result. See
//! `docs/adr/0071-native-context-menu-async-contract.md`.
//!
//! `popup_at_rect` (rather than `popup_at_pointer(None)`) needs a real
//! `GdkWindow` to anchor the menu to — tasty's window is owned by winit, not
//! GTK, so there is no `GdkWindow` for it by default. We wrap the winit
//! window's raw X11 XID as a foreign `GdkWindow` (same pattern as
//! `host_api/webview/linux.rs`) purely to give GTK a valid display/screen
//! context to position and grab from.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Mutex;
use std::time::Duration;

use gtk::glib::Cast;
use gtk::prelude::*;
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

use super::{MenuItem, MenuOutcome};

/// Watchdog bound. With the async contract this no longer guards against the
/// app freezing (nothing blocks any more) — it guards against a *ghost menu*:
/// if the grab fails and the user never clicks the menu itself, nothing would
/// ever fire `selection-done` and the popup would sit on screen forever with
/// the caller's continuation pinned behind it.
const WATCHDOG: Duration = Duration::from_secs(30);

fn ensure_gtk() -> bool {
    if gtk::is_initialized() {
        return true;
    }
    match gtk::init() {
        Ok(_) => true,
        Err(e) => {
            tracing::warn!("gtk::init failed: {e}");
            false
        }
    }
}

/// debug 훅: 워치독 타임아웃을 짧게 덮어써 grab 실패 경로를 실사용 대기 없이
/// 관찰한다 (`TASTY_DEBUG_NATIVE_MENU_TIMEOUT_MS`). release 미노출.
#[cfg(debug_assertions)]
fn watchdog_duration() -> Duration {
    std::env::var("TASTY_DEBUG_NATIVE_MENU_TIMEOUT_MS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .map_or(WATCHDOG, Duration::from_millis)
}

#[cfg(not(debug_assertions))]
fn watchdog_duration() -> Duration {
    WATCHDOG
}

/// debug 훅: grab 을 시도조차 하지 않아 "grab 실패" 상태를 결정적으로 만든다
/// (`TASTY_DEBUG_NATIVE_MENU_FORCE_GRAB_FAIL`). 실제 grab 실패는 물리 마우스
/// 클릭에서만 재현되므로(합성 입력으로는 안 됨) 이 경로가 유일한 자동 검증
/// 수단이다. release 미노출.
#[cfg(debug_assertions)]
fn force_grab_failure() -> bool {
    std::env::var_os("TASTY_DEBUG_NATIVE_MENU_FORCE_GRAB_FAIL").is_some()
}

#[cfg(not(debug_assertions))]
fn force_grab_failure() -> bool {
    false
}

/// An on-screen GTK popup menu whose result has not been collected yet.
///
/// Owns everything the popup's lifetime depends on: the `gtk::Menu` itself
/// (a function local in the old blocking implementation), the shared cells the
/// signal handlers write to, the watchdog source, and the X11 display the
/// grab must be released on.
pub struct GtkMenuHandle {
    menu: gtk::Menu,
    display: gdkx11::X11Display,
    selected: Rc<Cell<Option<u32>>>,
    done: Rc<Cell<bool>>,
    timed_out: Rc<Cell<bool>>,
    grabbed: Rc<Cell<bool>>,
    timeout_id: Option<gtk::glib::SourceId>,
    watchdog: Duration,
    finished: bool,
}

impl GtkMenuHandle {
    /// Service the menu once and report whether it closed.
    ///
    /// Non-blocking by construction: `main_iteration_do(false)` returns
    /// immediately when nothing is queued, so this drains what GTK already has
    /// and hands control straight back to the caller's frame loop.
    pub(super) fn poll(&mut self) -> Option<Option<u32>> {
        while gtk::events_pending() {
            gtk::main_iteration_do(false);
        }
        if !self.done.get() {
            return None;
        }
        Some(self.finish())
    }

    /// Close the menu without a selection (outside click routed from winit).
    /// The cancel surfaces through the next `poll`, so completion keeps a
    /// single path.
    pub(super) fn dismiss(&mut self) {
        if self.done.get() {
            return;
        }
        self.menu.popdown();
        self.done.set(true);
    }

    fn finish(&mut self) -> Option<u32> {
        let was_grabbed = self.grabbed.get();
        self.release();
        self.finished = true;
        if self.timed_out.get() {
            // 과거엔 grab 상태를 조회하지 않고 "likely a pointer grab failure"
            // 라고 단정했다 — 실제 `grabbed` 값을 찍어 추정과 사실을 구분한다.
            tracing::warn!(
                "native context menu popup timed out after {:?} without selection-done (pointer grab was {}) — forcing close",
                self.watchdog,
                if was_grabbed {
                    "established"
                } else {
                    "NOT established"
                }
            );
            return None;
        }
        self.selected.get()
    }

    /// Watchdog 해제 + 대칭 ungrab. 완료 경로(`finish`)와 미완 상태로 버려지는
    /// 경로(`Drop`)가 공유한다.
    fn release(&mut self) {
        if let Some(id) = self.timeout_id.take() {
            // 이미 발화한 once 소스는 스스로 제거된다 — 재제거는 glib 경고.
            if !self.timed_out.get() {
                id.remove();
            }
        }
        if self.grabbed.replace(false)
            && let Some(seat) = self
                .display
                .upcast_ref::<gtk::gdk::Display>()
                .default_seat()
        {
            seat.ungrab();
        }
    }
}

impl Drop for GtkMenuHandle {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        // 결과를 회수하지 않고 핸들이 버려지는 경로(창 종료 등) — 유령 메뉴와
        // 잡힌 채 남는 포인터 grab 을 남기지 않는다.
        self.menu.popdown();
        self.release();
    }
}

/// 이미 경고한 `(winit 배율 비트, GDK 배율)` 조합.
///
/// 우클릭마다 로그가 뜨면 그 줄은 읽히지 않는다. 그렇다고 프로세스당 한 번
/// (`Once`)으로 막으면 **배율이 다른 모니터로 창을 옮겨 어긋남의 모양이 바뀐 것**
/// 을 놓치는데, 그 변화가 정확히 진단에 필요한 값이다. 그래서 횟수가 아니라
/// **조합**으로 막는다 — 로그 줄 수는 우클릭 수가 아니라 서로 다른 배율 조합의
/// 수(모니터 수 정도)에 비례한다. `f64` 는 비트로 넣어 정확 비교한다.
static WARNED_ANCHOR_SCALES: Mutex<Vec<(u64, i32)>> = Mutex::new(Vec::new());

/// 위 경고-억제 셋 락의 poison 복구 공용 보고 좌표(첫-1 회). 복구는 안전하다(억제는 부가).
const WARNED_ANCHOR_SCALES_WHAT: &str = "native-menu anchor-scale warning set";
static WARNED_ANCHOR_SCALES_POISONED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// 네이티브 메뉴 앵커 좌표계의 **전제**(winit 배율 == GDK 배율)가 깨졌으면 경고한다.
///
/// [`show_context_menu`] 에 넘기는 `x`/`y` 는 winit(=egui) **논리** 좌표다. GTK 는
/// 같은 수를 `popup_at_rect` 에서 **GDK 논리** 좌표로 읽고 GDK 배율로 물리에
/// 올린다. 두 배율이 같을 때만 그 수가 같은 점을 가리킨다 — 즉 이 좌표 전달을
/// 맞게 만드는 것은 산술이 아니라 **전제**이고, 그 전제는 어디서도 강제되지
/// 않는다. winit 은 `WINIT_X11_SCALE_FACTOR`/Xft.dpi 를, GDK 는 `GDK_SCALE` 을
/// 서로 **다른 출처**에서 읽기 때문이다.
///
/// 전제가 깨지면 메뉴는 클릭 지점이 아니라 `winit 배율 / GDK 배율` 만큼 옮겨진
/// 자리에 뜬다(실측: winit 2 · GDK 1 에서 클릭 `(500,96)` → 메뉴 `+250+48`).
/// 산술을 고치지 않는 이유는 고칠 수 없어서가 아니라, **실기기 HiDPI X11 에서
/// 두 값이 실제로 갈리는지**를 확정하지 못해 어느 쪽으로 맞출지가 정해지지 않기
/// 때문이다. 그래서 전제를 주석으로만 두지 않고 깨진 순간을 로그로 남긴다 —
/// 그 관측은 실사용자 환경에서만 만들어진다.
///
/// 측정 절차와 실측값: `docs/ai-verification/dpi-scale-verification.md`
/// ("네이티브 메뉴 앵커는 winit 배율 == GDK 배율을 전제한다").
pub fn warn_if_menu_anchor_scale_premise_broken(winit_scale: f64) {
    if !ensure_gtk() {
        return;
    }
    let Some(display) = gtk::gdk::Display::default() else {
        return;
    };
    // GDK3/X11 의 배율은 디스플레이 전역(`GDK_SCALE`)이라 어느 모니터로 읽어도
    // 같다. primary 가 없는 구성만 0 번으로 물러선다.
    let Some(monitor) = display.primary_monitor().or_else(|| display.monitor(0)) else {
        return;
    };
    let gdk_scale = monitor.scale_factor();
    if winit_scale == f64::from(gdk_scale) {
        return;
    }

    let key = (winit_scale.to_bits(), gdk_scale);
    {
        // 경고 경로가 락 오염으로 침묵하면 안 된다 — 중복 억제는 부가 기능이다.
        let mut warned = crate::poison::recover_mutex(
            WARNED_ANCHOR_SCALES.lock(),
            WARNED_ANCHOR_SCALES_WHAT,
            &WARNED_ANCHOR_SCALES_POISONED,
        );
        if warned.contains(&key) {
            return;
        }
        warned.push(key);
    }

    tracing::warn!(
        "native context menu: winit scale_factor={winit_scale} vs GDK scale_factor={gdk_scale} \
         — 배율 전제가 깨졌다. 네이티브 메뉴 앵커가 어긋날 수 있다 \
         (메뉴가 클릭 지점에서 winit/GDK 배율비만큼 옮겨진 자리에 뜬다). \
         절차: docs/ai-verification/dpi-scale-verification.md"
    );
}

#[allow(clippy::cognitive_complexity)] // complexity-exempt: GTK/X11 grab 타이밍이 selected/done/grabbed/timed_out 4개 Rc<Cell<_>>를 공유하는 여러 클로저(activate/selection-done/button-press/idle/timeout)의 등록 순서 자체에 의미론이 있음(GDK Seat::grab을 idle 콜백에서 호출해야 하고 timeout이 없으면 유령 메뉴가 영원히 남음) — 클로저를 분리하면 각 함수가 4~5개 Rc<Cell<>> 핸들을 인자로 주고받아야 하고 실행 순서와 코드 위치가 물리적으로 분리되어 가독성이 오히려 나빠짐
pub fn show_context_menu(
    window: &impl HasWindowHandle,
    x: f64,
    y: f64,
    items: &[MenuItem],
) -> MenuOutcome {
    if !ensure_gtk() {
        return MenuOutcome::Ready(None);
    }

    let x11_window = match window.window_handle().ok().map(|h| h.as_raw()) {
        Some(RawWindowHandle::Xlib(w)) => w.window,
        _ => {
            tracing::warn!("native context menu: not an X11 window (Wayland is not supported)");
            return MenuOutcome::Ready(None);
        }
    };
    let gdk_display = match gtk::gdk::Display::default() {
        Some(d) => d,
        None => {
            tracing::warn!("native context menu: no GDK display");
            return MenuOutcome::Ready(None);
        }
    };
    let x11_gdk_display: gdkx11::X11Display = match gdk_display.downcast() {
        Ok(d) => d,
        Err(_) => {
            tracing::warn!("native context menu: GDK display is not X11");
            return MenuOutcome::Ready(None);
        }
    };
    // Foreign reference to tasty's own (winit-owned) window — not a new
    // window, just enough of a `GdkWindow` for popup_at_rect to anchor to.
    // 이 창은 winit 이 오래 전에 만든 것이라 webview 쪽과 달리 생성 경합은 없다.
    // 그래도 NULL 은 올 수 있고(종료 중 창이 이미 파괴된 경우), 그때 우클릭 하나로
    // 프로세스가 죽으면 안 된다 — 이 함수의 다른 실패 분기와 같이 메뉴를 안 띄운다.
    let rect_window =
        match crate::platform::x11_gdk_window::foreign_gdk_window(&x11_gdk_display, x11_window) {
            Ok(w) => w,
            Err(e) => {
                tracing::warn!("native context menu: {e}");
                return MenuOutcome::Ready(None);
            }
        };

    let menu = gtk::Menu::new();
    let selected: Rc<Cell<Option<u32>>> = Rc::new(Cell::new(None));

    for item in items {
        if item.is_separator() {
            let sep = gtk::SeparatorMenuItem::new();
            menu.append(&sep);
        } else {
            let mi = gtk::MenuItem::with_label(&item.label);
            mi.set_sensitive(item.enabled);
            if item.enabled {
                let id = item.id;
                let selected = Rc::clone(&selected);
                mi.connect_activate(move |_| {
                    selected.set(Some(id));
                });
            }
            menu.append(&mi);
        }
    }
    menu.show_all();

    let done: Rc<Cell<bool>> = Rc::new(Cell::new(false));
    {
        let done = Rc::clone(&done);
        menu.connect_selection_done(move |_| {
            done.set(true);
        });
    }

    // Explicit outside-click dismiss. GTK's own menu-shell deactivate logic
    // apparently keys off its own bookkeeping of "do I hold a grab", which
    // isn't reliably set here (no trigger `GdkEvent` to grab a timestamp
    // from — winit already consumed it) even though a grab does get
    // established (see below) — so don't depend on it. Instead watch
    // button-press-events on the menu directly: any press whose coordinates
    // land outside the menu's own allocation must be one redirected here by
    // the grab below (a real in-menu click is, by definition, inside it) —
    // treat that as "clicked outside" and dismiss ourselves.
    //
    // This is the *grabbed* dismiss path; when the grab fails the press never
    // reaches GTK at all and winit sees it instead — `mouse.rs` then calls
    // `MenuHandle::dismiss` on the handle returned below.
    {
        let done = Rc::clone(&done);
        menu.connect_button_press_event(move |menu_widget, event| {
            let (px, py) = event.position();
            let w = f64::from(menu_widget.allocated_width());
            let h = f64::from(menu_widget.allocated_height());
            if px < 0.0 || py < 0.0 || px >= w || py >= h {
                menu_widget.popdown();
                done.set(true);
            }
            gtk::glib::Propagation::Proceed
        });
    }

    // Best-effort pointer/keyboard grab, via GDK's own `Seat::grab` (not
    // raw Xlib `XGrabPointer`) so the resulting events flow through GDK's
    // normal (XInput2-based) event pipeline and actually reach the
    // button-press-event handler above — a raw core-protocol Xlib grab
    // redirects clicks at the X11 level too, but GDK3's event source only
    // recognizes XInput2 events, so those redirected clicks never turned
    // into a `GdkEventButton` at all (confirmed empirically: the handler
    // above never fired for outside clicks under a raw Xlib grab).
    // Without a grab at all, clicks outside the menu route to whatever
    // window is under them (tasty's own main window) and the menu never
    // sees them — so the handler above never gets a chance to run.
    //
    // Deferred to an idle callback (rather than done inline, or in the
    // widget "map" signal) so it runs *after* `popup_at_rect` below has
    // fully mapped the popup server-side — "map" fires as part of GTK's own
    // default handler for the signal, before the underlying map request is
    // guaranteed flushed, and grabbing too early fails.
    let grabbed: Rc<Cell<bool>> = Rc::new(Cell::new(false));
    {
        let grabbed = Rc::clone(&grabbed);
        let x11_gdk_display = x11_gdk_display.clone();
        let menu_weak = menu.downgrade();
        gtk::glib::idle_add_local_once(move || {
            if force_grab_failure() {
                tracing::warn!(
                    "native context menu: grab skipped (TASTY_DEBUG_NATIVE_MENU_FORCE_GRAB_FAIL)"
                );
                return;
            }
            let Some(menu) = menu_weak.upgrade() else {
                return;
            };
            let Some(gdk_win) = menu.window() else {
                return;
            };
            let Some(seat) = x11_gdk_display
                .upcast_ref::<gtk::gdk::Display>()
                .default_seat()
            else {
                return;
            };
            // `popup_at_rect`'s own internal grab (established with no
            // trigger event) does succeed at the X11 level — release it
            // first so our explicit one below doesn't fail with
            // `AlreadyGrabbed`.
            seat.ungrab();
            let status = seat.grab(
                &gdk_win,
                gtk::gdk::SeatCapabilities::POINTER | gtk::gdk::SeatCapabilities::KEYBOARD,
                true, // owner_events: let clicks inside the menu (or any of
                // its own sub-windows) route normally so GTK's own
                // item hit-testing/activation keeps working; clicks
                // outside every owned window still land on the menu
                // (the grab window) and reach the handler above.
                None,
                None,
                None,
            );
            if status == gtk::gdk::GrabStatus::Success {
                grabbed.set(true);
            } else {
                tracing::warn!(
                    "native context menu: seat grab failed ({status:?}) — outside-click dismiss falls back to the winit press path"
                );
            }
        });
    }

    let rect = gtk::gdk::Rectangle::new(x as i32, y as i32, 1, 1);
    menu.popup_at_rect(
        &rect_window,
        &rect,
        gtk::gdk::Gravity::NorthWest,
        gtk::gdk::Gravity::NorthWest,
        None,
    );

    // Safety net: without a real trigger event, `popup_at_rect` can still
    // fail to establish a pointer/keyboard grab (no timestamp to grab with)
    // under some window-manager / XWayland combinations, in which case
    // `selection-done` never fires. Nothing blocks any more, so this is no
    // longer a freeze guard — it stops a menu nobody can dismiss from sitting
    // on screen (and its continuation from being pinned) indefinitely.
    let watchdog = watchdog_duration();
    let timed_out: Rc<Cell<bool>> = Rc::new(Cell::new(false));
    let timeout_id = {
        let done = Rc::clone(&done);
        let timed_out = Rc::clone(&timed_out);
        let menu_weak = menu.downgrade();
        gtk::glib::timeout_add_local_once(watchdog, move || {
            if done.get() {
                return;
            }
            timed_out.set(true);
            done.set(true);
            if let Some(menu) = menu_weak.upgrade() {
                menu.popdown();
            }
        })
    };

    // Hand the live popup to the caller — no waiting here. The caller pumps
    // `poll()` each frame; winit's event loop keeps running the whole time.
    MenuOutcome::Pending(super::MenuHandle::from_gtk(GtkMenuHandle {
        menu,
        display: x11_gdk_display,
        selected,
        done,
        timed_out,
        grabbed,
        timeout_id: Some(timeout_id),
        watchdog,
        finished: false,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 테스트에서만 쓰는 X11 윈도우 핸들 어댑터 — GTK 가 만든 실제 창의 XID 를
    /// `show_context_menu` 가 요구하는 `HasWindowHandle` 로 감싼다.
    struct XlibWindow(std::os::raw::c_ulong);

    impl HasWindowHandle for XlibWindow {
        fn window_handle(
            &self,
        ) -> Result<winit::raw_window_handle::WindowHandle<'_>, winit::raw_window_handle::HandleError>
        {
            let h = winit::raw_window_handle::XlibWindowHandle::new(self.0);
            // SAFETY: 아래 호출부가 GTK 창을 이 핸들의 수명 동안 살려 둔다.
            Ok(unsafe {
                winit::raw_window_handle::WindowHandle::borrow_raw(RawWindowHandle::Xlib(h))
            })
        }
    }

    /// grab 실패를 강제한 상태에서도 (a) `show_context_menu` 가 즉시 반환하고
    /// (b) 폴링 한 번 한 번이 블로킹하지 않으며 (c) 워치독이 메뉴를 확실히
    /// 걷어간다는 것을 실제 GTK 백엔드로 확인한다.
    ///
    /// 실행: `cargo test --bin tasty -- --ignored --test-threads=1
    /// native_menu::linux` (X11 디스플레이 필요 — 잠깐 실제 메뉴가 떴다 사라진다).
    /// 물리 마우스 없이 재현 가능한 유일한 grab-실패 경로다.
    #[test]
    #[ignore]
    fn forced_grab_failure_resolves_via_watchdog_without_blocking() {
        // 가드가 원값 복원까지 맡는다 — 아래 단언 중 하나가 패닉해도 env 오염이
        // 남지 않는다. 동시 경합은 `#[ignore]` + `--test-threads=1` 실행 조건이 막는다.
        let _force_fail =
            crate::test_support::EnvVarGuard::set("TASTY_DEBUG_NATIVE_MENU_FORCE_GRAB_FAIL", "1");
        let _timeout =
            crate::test_support::EnvVarGuard::set("TASTY_DEBUG_NATIVE_MENU_TIMEOUT_MS", "250");
        assert!(ensure_gtk(), "이 테스트는 X11 디스플레이가 있어야 한다");

        let win = gtk::Window::new(gtk::WindowType::Toplevel);
        win.set_default_size(200, 100);
        win.show();
        while gtk::events_pending() {
            gtk::main_iteration_do(false);
        }
        let xid = win
            .window()
            .and_then(|w| w.downcast::<gdkx11::X11Window>().ok())
            .map(|w| w.xid())
            .expect("X11 GdkWindow");
        let anchor = XlibWindow(xid);

        let items = [MenuItem::new(1, "item")];
        let opened = std::time::Instant::now();
        let outcome = show_context_menu(&anchor, 10.0, 10.0, &items);
        assert!(
            opened.elapsed() < Duration::from_millis(500),
            "show_context_menu 는 메뉴가 닫히기를 기다리지 않는다"
        );
        let mut handle = match outcome {
            MenuOutcome::Pending(h) => h,
            MenuOutcome::Ready(_) => panic!("Linux 백엔드는 Pending 을 돌려줘야 한다"),
        };

        // 워치독이 걷어갈 때까지 폴링 — 각 폴링이 프레임 예산 안에 끝나야 한다.
        let mut polls = 0u32;
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let result = loop {
            let tick = std::time::Instant::now();
            let r = handle.poll();
            assert!(
                tick.elapsed() < Duration::from_millis(100),
                "폴링 한 번이 {:?} 나 걸렸다 — 블로킹 대기가 되살아났다",
                tick.elapsed()
            );
            polls += 1;
            if let Some(r) = r {
                break r;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "250ms 워치독이 5초 안에 메뉴를 걷어가지 않았다"
            );
            std::thread::sleep(Duration::from_millis(5));
        };
        assert_eq!(result, None, "워치독 강제 종료는 선택 없음으로 해소된다");
        assert!(
            polls > 1,
            "메뉴가 떠 있는 동안 호출자가 여러 번 제어를 돌려받아야 한다 (polls={polls})"
        );

        win.close();
    }
}
