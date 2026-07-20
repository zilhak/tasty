//! Linux native context menu using GTK 3 Menu + popup_at_rect (X11 only).
//!
//! GTK is initialized lazily on first call. The menu blocks the caller until
//! dismissed by spinning `gtk::main_iteration_do` until `selection-done`
//! fires — keeping the contract identical to the macOS / Windows backends
//! (synchronous, returns the selected item id or `None` on cancel).
//!
//! `popup_at_rect` (rather than `popup_at_pointer(None)`) needs a real
//! `GdkWindow` to anchor the menu to — tasty's window is owned by winit, not
//! GTK, so there is no `GdkWindow` for it by default. We wrap the winit
//! window's raw X11 XID as a foreign `GdkWindow` (same pattern as
//! `host_api/webview/linux.rs`) purely to give GTK a valid display/screen
//! context to position and grab from.

use std::cell::Cell;
use std::rc::Rc;

use gtk::glib::Cast;
use gtk::prelude::*;
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

use super::MenuItem;

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

pub fn show_context_menu(
    window: &impl HasWindowHandle,
    x: f64,
    y: f64,
    items: &[MenuItem],
) -> Option<u32> {
    if !ensure_gtk() {
        return None;
    }

    let x11_window = match window.window_handle().ok().map(|h| h.as_raw()) {
        Some(RawWindowHandle::Xlib(w)) => w.window,
        _ => {
            tracing::warn!("native context menu: not an X11 window (Wayland is not supported)");
            return None;
        }
    };
    let gdk_display = match gtk::gdk::Display::default() {
        Some(d) => d,
        None => {
            tracing::warn!("native context menu: no GDK display");
            return None;
        }
    };
    let x11_gdk_display: gdkx11::X11Display = match gdk_display.downcast() {
        Ok(d) => d,
        Err(_) => {
            tracing::warn!("native context menu: GDK display is not X11");
            return None;
        }
    };
    // Foreign reference to tasty's own (winit-owned) window — not a new
    // window, just enough of a `GdkWindow` for popup_at_rect to anchor to.
    let rect_window: gtk::gdk::Window =
        gdkx11::X11Window::foreign_new_for_display(&x11_gdk_display, x11_window).upcast();

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
                    "native context menu: seat grab failed ({status:?}) — outside-click dismiss may not work, relying on the timeout fallback"
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
    // `selection-done` never fires and the loop below would spin forever,
    // freezing the whole app until force-killed. Force the popup closed
    // after a generous bound so a broken grab degrades to "clicking outside
    // the menu doesn't dismiss it for a while" instead of an unrecoverable
    // hang.
    let timed_out: Rc<Cell<bool>> = Rc::new(Cell::new(false));
    let timeout_id = {
        let done = Rc::clone(&done);
        let timed_out = Rc::clone(&timed_out);
        let menu_weak = menu.downgrade();
        gtk::glib::timeout_add_local_once(std::time::Duration::from_secs(30), move || {
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

    // Spin GTK's main loop until the menu is dismissed (selection or cancel).
    // `main_iteration_do(true)` blocks for the next event so this idle-yields
    // instead of busy-looping.
    while !done.get() {
        gtk::main_iteration_do(true);
    }
    if !timed_out.get() {
        timeout_id.remove();
    }

    // Symmetric ungrab on every exit path (selection, cancel, or timeout).
    if grabbed.get() {
        if let Some(seat) = x11_gdk_display
            .upcast_ref::<gtk::gdk::Display>()
            .default_seat()
        {
            seat.ungrab();
        }
    }

    if timed_out.get() {
        tracing::warn!(
            "native context menu popup timed out after 30s without selection-done (likely a pointer grab failure) — forcing close"
        );
        return None;
    }

    selected.get()
}
