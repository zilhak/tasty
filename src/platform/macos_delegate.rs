//! macOS dock reopen, dock menu, and app menu support.
//!
//! Instead of replacing winit's NSApplicationDelegate (which breaks winit),
//! we inject methods directly into winit's existing delegate class at runtime.
//! This is called after winit has set up its delegate (in `resumed()`).

use std::sync::OnceLock;

use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Bool, Sel};
use objc2::{AnyThread, msg_send, sel};
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
    // sel! 매크로 내부 + setAction 호출이 한 묶음.
    #[allow(clippy::multiple_unsafe_ops_per_block)]
    unsafe {
        item.setAction(Some(sel!(tastyNewWindow:)))
    };
    menu.addItem(&item);

    let ptr: *mut NSMenu = Retained::into_raw(menu);
    // SAFETY: autorelease는 NSObject 표준 메서드 — main thread 호출이므로 ObjC runtime 안전.
    // msg_send 매크로 내부 + 호출이 한 묶음.
    #[allow(clippy::multiple_unsafe_ops_per_block)]
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

/// `tastyQuit:` action handler — winit 자동 `terminate:` 대신 tasty 라이프사이클로 라우팅.
unsafe extern "C-unwind" fn quit_action(_this: *mut AnyObject, _sel: Sel, _sender: *mut AnyObject) {
    tracing::info!("menu: quit requested");
    if let Some(proxy) = PROXY.get() {
        crate::shortcuts::send_app_event(proxy, AppEvent::QuitRequested);
    }
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

        objc2::ffi::class_addMethod(
            cls,
            sel!(tastyQuit:),
            std::mem::transmute::<
                unsafe extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject),
                objc2::runtime::Imp,
            >(quit_action),
            c"v@:@".as_ptr(),
        );
    }

    // Set up app menu
    // SAFETY: delegate는 Retained<dyn>, msg_send![,self]는 self pointer를 얻는 ObjC 표준 호출.
    // main thread에서 호출됨.
    #[allow(clippy::multiple_unsafe_ops_per_block)]
    let delegate_ptr: *mut AnyObject = unsafe { msg_send![&*delegate, self] };
    setup_main_menu(&app, mtm, delegate_ptr);

    // Set Dock icon from embedded PNG (works even without .app bundle)
    set_dock_icon(&app);

    tracing::info!("macOS delegate methods injected into winit's delegate");
}

/// macOS 표준 menubar 등록. winit `with_default_menu(false)` 와 짝.
///
/// Application Menu (About/Hide/.../Quit), File (New Window), Edit (Cut/Copy/Paste/
/// Select All), Window (Minimize/Zoom/Close Window) 4개 submenu 를 등록한다.
/// 표준 selector 는 first responder chain 으로 전달 (target=nil), `tastyQuit:` /
/// `tastyNewWindow:` 만 delegate target 지정.
fn setup_main_menu(app: &NSApplication, mtm: MainThreadMarker, delegate: *mut AnyObject) {
    use objc2_app_kit::NSEventModifierFlags;
    use objc2_foundation::NSProcessInfo;

    let main_menu = NSMenu::new(mtm);
    let process_name = NSProcessInfo::processInfo().processName();

    // ── Application Menu ──────────────────────────────────────────
    let app_menu_item = NSMenuItem::new(mtm);
    let app_menu = NSMenu::new(mtm);

    let about_title = NSString::from_str("About ").stringByAppendingString(&process_name);
    app_menu.addItem(&make_std_item(
        mtm,
        &about_title,
        Some(sel!(orderFrontStandardAboutPanel:)),
        None,
        NSEventModifierFlags::Command,
    ));
    app_menu.addItem(&NSMenuItem::separatorItem(mtm));
    let hide_title = NSString::from_str("Hide ").stringByAppendingString(&process_name);
    app_menu.addItem(&make_std_item(
        mtm,
        &hide_title,
        Some(sel!(hide:)),
        Some("h"),
        NSEventModifierFlags::Command,
    ));
    app_menu.addItem(&make_std_item(
        mtm,
        &NSString::from_str("Hide Others"),
        Some(sel!(hideOtherApplications:)),
        Some("h"),
        NSEventModifierFlags::Command | NSEventModifierFlags::Option,
    ));
    app_menu.addItem(&make_std_item(
        mtm,
        &NSString::from_str("Show All"),
        Some(sel!(unhideAllApplications:)),
        None,
        NSEventModifierFlags::Command,
    ));
    app_menu.addItem(&NSMenuItem::separatorItem(mtm));

    // Quit — tasty 라이프사이클로 라우팅.
    let quit_title = NSString::from_str("Quit ").stringByAppendingString(&process_name);
    let quit_item = NSMenuItem::new(mtm);
    quit_item.setTitle(&quit_title);
    quit_item.setKeyEquivalent(&NSString::from_str("q"));
    // SAFETY: main thread (mtm). delegate 는 NSApplicationDelegate 포인터로 app 수명 동안 유효.
    // setAction / setTarget / setKeyEquivalentModifierMask 한 단위.
    #[allow(clippy::multiple_unsafe_ops_per_block)]
    unsafe {
        quit_item.setAction(Some(sel!(tastyQuit:)));
        quit_item.setTarget(Some(&*delegate.cast()));
        quit_item.setKeyEquivalentModifierMask(NSEventModifierFlags::Command);
    }
    app_menu.addItem(&quit_item);

    app_menu_item.setSubmenu(Some(&app_menu));
    main_menu.addItem(&app_menu_item);

    // ── File Menu ──────────────────────────────────────────────────
    let file_menu_item = NSMenuItem::new(mtm);
    let file_menu = NSMenu::new(mtm);
    file_menu.setTitle(&NSString::from_str("File"));

    let new_window_item = NSMenuItem::new(mtm);
    new_window_item.setTitle(&NSString::from_str("New Window"));
    new_window_item.setKeyEquivalent(&NSString::from_str("n"));
    // SAFETY: 위 quit_item 와 동일 — main thread + delegate 수명 보장.
    #[allow(clippy::multiple_unsafe_ops_per_block)]
    unsafe {
        new_window_item.setAction(Some(sel!(tastyNewWindow:)));
        new_window_item.setTarget(Some(&*delegate.cast()));
        new_window_item.setKeyEquivalentModifierMask(
            NSEventModifierFlags::Command | NSEventModifierFlags::Shift,
        );
    }
    file_menu.addItem(&new_window_item);

    file_menu_item.setSubmenu(Some(&file_menu));
    main_menu.addItem(&file_menu_item);

    // ── Edit Menu ──────────────────────────────────────────────────
    let edit_menu_item = NSMenuItem::new(mtm);
    let edit_menu = NSMenu::new(mtm);
    edit_menu.setTitle(&NSString::from_str("Edit"));
    edit_menu.addItem(&make_std_item(
        mtm,
        &NSString::from_str("Cut"),
        Some(sel!(cut:)),
        Some("x"),
        NSEventModifierFlags::Command,
    ));
    edit_menu.addItem(&make_std_item(
        mtm,
        &NSString::from_str("Copy"),
        Some(sel!(copy:)),
        Some("c"),
        NSEventModifierFlags::Command,
    ));
    edit_menu.addItem(&make_std_item(
        mtm,
        &NSString::from_str("Paste"),
        Some(sel!(paste:)),
        Some("v"),
        NSEventModifierFlags::Command,
    ));
    edit_menu.addItem(&make_std_item(
        mtm,
        &NSString::from_str("Select All"),
        Some(sel!(selectAll:)),
        Some("a"),
        NSEventModifierFlags::Command,
    ));
    edit_menu_item.setSubmenu(Some(&edit_menu));
    main_menu.addItem(&edit_menu_item);

    // ── Window Menu ────────────────────────────────────────────────
    let window_menu_item = NSMenuItem::new(mtm);
    let window_menu = NSMenu::new(mtm);
    window_menu.setTitle(&NSString::from_str("Window"));
    window_menu.addItem(&make_std_item(
        mtm,
        &NSString::from_str("Minimize"),
        Some(sel!(miniaturize:)),
        Some("m"),
        NSEventModifierFlags::Command,
    ));
    window_menu.addItem(&make_std_item(
        mtm,
        &NSString::from_str("Zoom"),
        Some(sel!(performZoom:)),
        None,
        NSEventModifierFlags::Command,
    ));
    window_menu.addItem(&make_std_item(
        mtm,
        &NSString::from_str("Close Window"),
        Some(sel!(performClose:)),
        Some("w"),
        NSEventModifierFlags::Command,
    ));
    window_menu_item.setSubmenu(Some(&window_menu));
    main_menu.addItem(&window_menu_item);

    app.setMainMenu(Some(&main_menu));
}

/// 표준 NSResponder selector 용 NSMenuItem 생성 (target = nil → first responder chain).
fn make_std_item(
    mtm: MainThreadMarker,
    title: &NSString,
    selector: Option<Sel>,
    key: Option<&str>,
    modifier_mask: objc2_app_kit::NSEventModifierFlags,
) -> Retained<NSMenuItem> {
    let item = NSMenuItem::new(mtm);
    item.setTitle(title);
    if let Some(key_str) = key {
        item.setKeyEquivalent(&NSString::from_str(key_str));
        // SAFETY: main thread (mtm). setKeyEquivalentModifierMask 는 AppKit main-thread-only.
        unsafe {
            item.setKeyEquivalentModifierMask(modifier_mask);
        }
    }
    if let Some(sel) = selector {
        // SAFETY: main thread (mtm). target 미설정 = nil = first responder chain (표준 selector 용).
        unsafe {
            item.setAction(Some(sel));
        }
    }
    item
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
