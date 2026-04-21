//! macOS dock reopen, dock menu, and app menu support.
//!
//! Instead of replacing winit's NSApplicationDelegate (which breaks winit),
//! we inject methods directly into winit's existing delegate class at runtime.
//! This is called after winit has set up its delegate (in `resumed()`).

use std::sync::OnceLock;

use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Bool, Sel};
use objc2::{msg_send, sel};
use objc2_app_kit::{NSApplication, NSMenu, NSMenuItem};
use objc2_foundation::{MainThreadMarker, NSString};
use winit::event_loop::EventLoopProxy;

use crate::AppEvent;

/// Global proxy stored so ObjC callbacks can access it.
static PROXY: OnceLock<EventLoopProxy<AppEvent>> = OnceLock::new();

fn send_create_window() {
    if let Some(proxy) = PROXY.get() {
        let _ = proxy.send_event(AppEvent::CreateWindow);
    }
}

/// `applicationShouldHandleReopen:hasVisibleWindows:` callback
unsafe extern "C-unwind" fn handle_reopen(
    _this: *mut AnyObject,
    _sel: Sel,
    _sender: *mut AnyObject,
    has_visible_windows: Bool,
) -> Bool {
    if !has_visible_windows.as_bool() {
        tracing::info!("dock reopen: no visible windows, creating new window");
        send_create_window();
    }
    Bool::YES
}

/// `applicationDockMenu:` callback
unsafe extern "C-unwind" fn dock_menu(
    _this: *mut AnyObject,
    _sel: Sel,
    _sender: *mut AnyObject,
) -> *mut AnyObject {
    let Some(mtm) = MainThreadMarker::new() else {
        return std::ptr::null_mut();
    };

    let menu = NSMenu::new(mtm);
    let item = NSMenuItem::new(mtm);
    item.setTitle(&NSString::from_str("New Window"));
    unsafe { item.setAction(Some(sel!(tastyNewWindow:))) };
    menu.addItem(&item);

    let ptr: *mut NSMenu = Retained::into_raw(menu);
    let _: *mut AnyObject = msg_send![ptr, autorelease];
    ptr.cast()
}

/// `tastyNewWindow:` action handler
unsafe extern "C-unwind" fn new_window_action(
    _this: *mut AnyObject,
    _sel: Sel,
    _sender: *mut AnyObject,
) {
    tracing::info!("dock/menu: new window requested");
    send_create_window();
}

/// Store the proxy. Called once at startup (before run_app).
pub fn store_proxy(proxy: EventLoopProxy<AppEvent>) {
    PROXY.set(proxy).ok();
}

/// Inject delegate methods into winit's existing delegate class.
/// Must be called AFTER winit has set up its delegate (i.e., from `resumed()`).
pub fn inject_delegate_methods() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };

    let app = NSApplication::sharedApplication(mtm);
    let delegate = match app.delegate() {
        Some(d) => d,
        None => {
            tracing::warn!("macOS delegate inject: no delegate found");
            return;
        }
    };

    unsafe {
        // Get the class of winit's delegate and inject our methods into it.
        let cls: *mut AnyClass = msg_send![&*delegate, class];

        objc2::ffi::class_addMethod(
            cls,
            sel!(applicationShouldHandleReopen:hasVisibleWindows:),
            std::mem::transmute::<
                unsafe extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject, Bool) -> Bool,
                objc2::runtime::Imp,
            >(handle_reopen),
            c"B@:@B".as_ptr(),
        );

        objc2::ffi::class_addMethod(
            cls,
            sel!(applicationDockMenu:),
            std::mem::transmute::<
                unsafe extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject) -> *mut AnyObject,
                objc2::runtime::Imp,
            >(dock_menu),
            c"@@:@".as_ptr(),
        );

        objc2::ffi::class_addMethod(
            cls,
            sel!(tastyNewWindow:),
            std::mem::transmute::<
                unsafe extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject),
                objc2::runtime::Imp,
            >(new_window_action),
            c"v@:@".as_ptr(),
        );
    }

    // Set up app menu
    let delegate_ptr: *mut AnyObject = unsafe { msg_send![&*delegate, self] };
    setup_main_menu(&app, mtm, delegate_ptr);

    tracing::info!("macOS delegate methods injected into winit's delegate");
}

/// Set up the app menu bar with File → New Window.
fn setup_main_menu(app: &NSApplication, mtm: MainThreadMarker, delegate: *mut AnyObject) {
    let main_menu = match app.mainMenu() {
        Some(menu) => menu,
        None => {
            let menu = NSMenu::new(mtm);
            app.setMainMenu(Some(&menu));
            menu
        }
    };

    let file_menu = find_or_create_file_menu(&main_menu, mtm);

    let new_window_item = NSMenuItem::new(mtm);
    new_window_item.setTitle(&NSString::from_str("New Window"));
    unsafe {
        new_window_item.setAction(Some(sel!(tastyNewWindow:)));
        new_window_item.setTarget(Some(&*delegate.cast()));
        new_window_item.setKeyEquivalentModifierMask(
            objc2_app_kit::NSEventModifierFlags::Command
                | objc2_app_kit::NSEventModifierFlags::Shift,
        );
    }
    new_window_item.setKeyEquivalent(&NSString::from_str("n"));

    file_menu.insertItem_atIndex(&new_window_item, 0);
}

fn find_or_create_file_menu(main_menu: &NSMenu, mtm: MainThreadMarker) -> Retained<NSMenu> {
    let count = main_menu.numberOfItems();
    for i in 0..count {
        if let Some(item) = main_menu.itemAtIndex(i) {
            if let Some(submenu) = item.submenu() {
                let title = submenu.title().to_string();
                if title == "File" {
                    return submenu;
                }
            }
        }
    }

    let file_menu = NSMenu::new(mtm);
    file_menu.setTitle(&NSString::from_str("File"));

    let file_item = NSMenuItem::new(mtm);
    file_item.setSubmenu(Some(&file_menu));

    let insert_idx = if count > 0 { 1 } else { 0 };
    main_menu.insertItem_atIndex(&file_item, insert_idx);

    file_menu
}
