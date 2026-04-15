//! macOS NSApplicationDelegate for dock reopen, dock menu, and app menu.
//!
//! Uses raw Objective-C runtime APIs to avoid objc2 version conflicts
//! (winit uses objc2 0.5, we use 0.6).

use std::sync::OnceLock;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, Sel};
use objc2::{msg_send, sel, class};
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

    // Transfer ownership via autorelease
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

/// Register the delegate class at runtime and set it on NSApplication.
/// Must be called after EventLoop creation and before run_app.
pub fn setup(proxy: EventLoopProxy<AppEvent>) {
    PROXY.set(proxy).ok();

    let Some(mtm) = MainThreadMarker::new() else {
        tracing::warn!("macOS delegate setup: not on main thread, skipping");
        return;
    };

    unsafe {
        // Use raw ObjC runtime to build class, avoiding ClassBuilder's private API
        let superclass: *const objc2::runtime::AnyClass = class!(NSObject);
        let cls_ptr = objc2::ffi::objc_allocateClassPair(
            superclass.cast_mut(),
            c"TastyAppDelegate".as_ptr(),
            0,
        );
        assert!(!cls_ptr.is_null(), "failed to allocate TastyAppDelegate class");

        if let Some(protocol) = objc2::runtime::AnyProtocol::get(c"NSApplicationDelegate") {
            objc2::ffi::class_addProtocol(cls_ptr, protocol as *const _ as *mut _);
        }

        objc2::ffi::class_addMethod(
            cls_ptr,
            sel!(applicationShouldHandleReopen:hasVisibleWindows:),
            std::mem::transmute::<unsafe extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject, Bool) -> Bool, objc2::runtime::Imp>(handle_reopen),
            c"B@:@B".as_ptr(),
        );

        objc2::ffi::class_addMethod(
            cls_ptr,
            sel!(applicationDockMenu:),
            std::mem::transmute::<unsafe extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject) -> *mut AnyObject, objc2::runtime::Imp>(dock_menu),
            c"@@:@".as_ptr(),
        );

        objc2::ffi::class_addMethod(
            cls_ptr,
            sel!(tastyNewWindow:),
            std::mem::transmute::<unsafe extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject), objc2::runtime::Imp>(new_window_action),
            c"v@:@".as_ptr(),
        );

        objc2::ffi::objc_registerClassPair(cls_ptr);
        let cls = &*cls_ptr;

        // Create instance and set as delegate
        let delegate: *mut AnyObject = msg_send![cls, alloc];
        let delegate: *mut AnyObject = msg_send![delegate, init];

        let app = NSApplication::sharedApplication(mtm);
        let _: () = msg_send![&*app, setDelegate: delegate];

        setup_main_menu(&app, mtm, delegate);

        // delegate intentionally leaked — must outlive the app
    }
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
