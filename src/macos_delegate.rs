//! macOS dock reopen, dock menu, and app menu support.
//!
//! Instead of replacing winit's NSApplicationDelegate (which breaks winit),
//! we inject methods directly into winit's existing delegate class at runtime.
//! This is called after winit has set up its delegate (in `resumed()`).

use std::sync::OnceLock;

use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Bool, Sel};
use objc2::{msg_send, sel, AnyThread};
use objc2_app_kit::{NSApplication, NSMenu, NSMenuItem};
use objc2_foundation::{MainThreadMarker, NSString};
use winit::event_loop::EventLoopProxy;

use crate::AppEvent;

/// Global proxy stored so ObjC callbacks can access it.
static PROXY: OnceLock<EventLoopProxy<AppEvent>> = OnceLock::new();

fn send_create_window() {
    if let Some(proxy) = PROXY.get() {
        crate::shortcuts::send_app_event(proxy, AppEvent::CreateWindow);
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
    // SAFETY: 본 함수는 unsafe extern "C-unwind" — main thread NSApplicationDelegate
    // 콜백으로 ObjC 런타임이 호출. setAction은 AppKit main-thread-only이며 invariant 충족.
    unsafe { item.setAction(Some(sel!(tastyNewWindow:))) };
    menu.addItem(&item);

    let ptr: *mut NSMenu = Retained::into_raw(menu);
    // SAFETY: autorelease는 NSObject 표준 메서드 — main thread 호출이므로 ObjC runtime 안전.
    let _: *mut AnyObject = unsafe { msg_send![ptr, autorelease] };
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

    // SAFETY: 본 블록은 winit의 NSApplicationDelegate 클래스에 메서드 3개를 주입하는
    // class_addMethod 호출 시퀀스. 호출은 `resumed()` 흐름의 main thread에서만 수행된다
    // (MainThreadMarker로 위에서 검증). 주입하는 함수 포인터(handle_reopen 등)는 모두
    // 'static fn이므로 lifetime 안전. transmute는 unsafe extern fn → Imp (둘 다 raw fn ptr)
    // 캐스팅이며 ObjC runtime이 ABI-compatible로 받음을 objc2 문서가 보장. 시그니처 인코딩
    // 문자열(c"B@:@B" 등)은 'static C string.
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
    // SAFETY: delegate는 Retained<dyn>, msg_send![,self]는 self pointer를 얻는 ObjC 표준 호출.
    // main thread에서 호출됨.
    let delegate_ptr: *mut AnyObject = unsafe { msg_send![&*delegate, self] };
    setup_main_menu(&app, mtm, delegate_ptr);

    // Set Dock icon from embedded PNG (works even without .app bundle)
    set_dock_icon(&app);

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
    // SAFETY: 호출자가 main thread(mtm)에서 호출. delegate는 AnyObject 포인터로,
    // winit의 NSApplicationDelegate를 가리키며 app이 살아있는 동안 valid.
    // NSEventModifierFlags 조합은 bitflag로 ObjC API에서 받아들이는 표준 값.
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

/// Set the Dock icon using NSApplication::setApplicationIconImage.
/// This works even for non-bundled executables (cargo run).
fn set_dock_icon(app: &NSApplication) {
    use objc2_app_kit::NSImage;
    use objc2_foundation::NSData;

    let png_bytes = crate::app_icon::ICON_PNG_256;
    let data = NSData::with_bytes(png_bytes);
    let image = NSImage::initWithData(NSImage::alloc(), &data);
    if let Some(image) = image {
        // SAFETY: setApplicationIconImage은 main thread NSApplication 메서드. 호출자가 보장.
        unsafe { app.setApplicationIconImage(Some(&image)) };
    } else {
        tracing::warn!("Failed to create NSImage for dock icon");
    }
}
