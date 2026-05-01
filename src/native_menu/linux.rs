//! Linux native context menu using GTK 3 Menu + popup_at_pointer.
//!
//! GTK is initialized lazily on first call. The menu blocks the caller until
//! dismissed by spinning `gtk::main_iteration_do` until `selection-done`
//! fires — keeping the contract identical to the macOS / Windows backends
//! (synchronous, returns the selected item id or `None` on cancel).

use std::cell::Cell;
use std::rc::Rc;

use gtk::prelude::*;
use winit::raw_window_handle::HasWindowHandle;

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
    _window: &impl HasWindowHandle,
    _x: f64,
    _y: f64,
    items: &[MenuItem],
) -> Option<u32> {
    if !ensure_gtk() {
        return None;
    }

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

    menu.popup_at_pointer(None::<&gtk::gdk::Event>);

    // Spin GTK's main loop until the menu is dismissed (selection or cancel).
    // `main_iteration_do(true)` blocks for the next event so this idle-yields
    // instead of busy-looping.
    while !done.get() {
        gtk::main_iteration_do(true);
    }

    selected.get()
}
