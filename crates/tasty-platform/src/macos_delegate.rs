//! macOS dock reopen, dock menu, and app menu support.
//!
//! Instead of replacing winit's NSApplicationDelegate (which breaks winit),
//! we inject methods directly into winit's existing delegate class at runtime.
//! This is called after winit has set up its delegate (in `resumed()`).
//!
//! dock/menu 동작은 호출자가 DelegateActions로 전달한다.

use std::sync::OnceLock;

use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Bool, Sel};
use objc2::{AnyThread, msg_send, sel};
use objc2_app_kit::{NSApplication, NSMenu, NSMenuItem};
use objc2_foundation::{MainThreadMarker, NSString};

use tasty_i18n::{t, t_fmt};

/// dock/menu 콜백에서 사용할 동작. 전역 OnceLock으로 공유하므로 Send + Sync가 필요하다.
pub struct DelegateActions {
    /// dock reopen · dock 메뉴 · `tastyNewWindow:` 가 부른다.
    pub new_window: Box<dyn Fn() + Send + Sync + 'static>,
    /// `tastyQuit:` 가 부른다.
    pub quit: Box<dyn Fn() + Send + Sync + 'static>,
}

/// ObjC 콜백이 접근할 수 있도록 보관한 동작들.
static ACTIONS: OnceLock<DelegateActions> = OnceLock::new();

fn send_create_window() {
    if let Some(actions) = ACTIONS.get() {
        (actions.new_window)();
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
    item.setTitle(&NSString::from_str(t("menu.macos.new_window")));
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
    if let Some(actions) = ACTIONS.get() {
        (actions.quit)();
    }
}

/// Store the actions. Called once at startup (before run_app).
pub fn store_actions(actions: DelegateActions) {
    ACTIONS.set(actions).ok();
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

    // SAFETY: 본 블록은 winit의 NSApplicationDelegate 클래스에 메서드 4개를 주입하는
    // class_addMethod 호출 시퀀스. 호출은 `resumed()` 흐름의 main thread에서만 수행된다
    // (MainThreadMarker로 위에서 검증). 주입하는 함수 포인터(handle_reopen 등)는 모두
    // 'static fn이므로 lifetime 안전. transmute는 unsafe extern fn → Imp (둘 다 raw fn ptr)
    // 캐스팅이며 ObjC runtime이 ABI-compatible로 받음을 objc2 문서가 보장. 시그니처 인코딩
    // 문자열(c"B@:@B" 등)은 'static C string.
    #[allow(clippy::multiple_unsafe_ops_per_block)]
    unsafe {
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

    // SAFETY: delegate는 Retained<dyn>, msg_send![,self]는 self pointer를 얻는 ObjC 표준 호출.
    // main thread에서 호출됨.
    #[allow(clippy::multiple_unsafe_ops_per_block)]
    let delegate_ptr: *mut AnyObject = unsafe { msg_send![&*delegate, self] };
    // tasty 특화 액션의 key equivalent 는 KeybindingSettings 에서 가져온다 (부팅 시 1회).
    let settings = tasty_settings::Settings::load();
    setup_main_menu(&app, mtm, delegate_ptr, &settings.keybindings);

    set_dock_icon(&app);

    tracing::info!("macOS delegate methods injected into winit's delegate");
}

/// 키 설정 변경 뒤 메뉴를 다시 만든다. main thread가 아니면 경고하고 반환한다.
/// NSApplication과 delegate를 다시 얻고 새 항목에 target을 지정한다.
pub fn rebuild_main_menu(keybindings: &tasty_settings::KeybindingSettings) {
    let Some(mtm) = MainThreadMarker::new() else {
        tracing::warn!("rebuild_main_menu: not on main thread, skipping");
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    let Some(delegate) = app.delegate() else {
        tracing::warn!("rebuild_main_menu: NSApplication has no delegate yet");
        return;
    };
    // SAFETY: main thread (mtm 로 검증). msg_send self 는 NSObject 표준 메서드 —
    // delegate (Retained<dyn>) 가 살아있는 동안 안전. 결과 ptr 은 setup_main_menu
    // 의 setTarget 인자로만 사용.
    let delegate_ptr: *mut AnyObject = unsafe { msg_send![&*delegate, self] };
    setup_main_menu(&app, mtm, delegate_ptr, keybindings);
    tracing::debug!("macOS NSMenu rebuilt for KeybindingSettings change");
}

/// App/File/Window 메뉴를 만든다. OS 표준 액션은 target=nil로 responder chain에 전달하고
/// Quit/New Window는 Tasty delegate로 보낸다. Edit 동작은 호스트 키 디스패치가 처리한다.
/// 단축키는 KeybindingSettings의 첫 바인딩을 쓰거나 비워 두며 고정값을 넣지 않는다.
/// 라벨은 menu.macos.* 번역에서 매번 읽고 앱 이름은 t_fmt로 삽입한다.
fn setup_main_menu(
    app: &NSApplication,
    mtm: MainThreadMarker,
    delegate: *mut AnyObject,
    keybindings: &tasty_settings::KeybindingSettings,
) {
    use objc2_app_kit::NSEventModifierFlags;
    use objc2_foundation::NSProcessInfo;

    let main_menu = NSMenu::new(mtm);
    let process_name = NSProcessInfo::processInfo().processName().to_string();

    // ── Application Menu ──────────────────────────────────────────
    let app_menu_item = NSMenuItem::new(mtm);
    let app_menu = NSMenu::new(mtm);

    let about_title = t_fmt("menu.macos.about", &process_name);
    app_menu.addItem(&make_std_item(
        mtm,
        &about_title,
        sel!(orderFrontStandardAboutPanel:),
    ));
    app_menu.addItem(&NSMenuItem::separatorItem(mtm));
    let hide_title = t_fmt("menu.macos.hide", &process_name);
    app_menu.addItem(&make_std_item(mtm, &hide_title, sel!(hide:)));
    app_menu.addItem(&make_std_item(
        mtm,
        t("menu.macos.hide_others"),
        sel!(hideOtherApplications:),
    ));
    app_menu.addItem(&make_std_item(
        mtm,
        t("menu.macos.show_all"),
        sel!(unhideAllApplications:),
    ));
    app_menu.addItem(&NSMenuItem::separatorItem(mtm));

    // Quit — tasty 라이프사이클로 라우팅. key equivalent 는 KeybindingSettings.quit 에서.
    let quit_title = t_fmt("menu.macos.quit", &process_name);
    let quit_item = NSMenuItem::new(mtm);
    quit_item.setTitle(&NSString::from_str(&quit_title));
    let (quit_key, quit_mods) = keybindings
        .quit
        .first()
        .map(|b| binding_to_nsmenu_key(b))
        .unwrap_or_else(|| (NSString::from_str(""), NSEventModifierFlags::empty()));
    quit_item.setKeyEquivalent(&quit_key);
    // SAFETY: main thread (mtm). delegate 는 NSApplicationDelegate 포인터로 app 수명 동안 유효.
    // setAction / setTarget / setKeyEquivalentModifierMask 한 단위.
    #[allow(clippy::multiple_unsafe_ops_per_block)]
    unsafe {
        quit_item.setAction(Some(sel!(tastyQuit:)));
        quit_item.setTarget(Some(&*delegate.cast()));
        quit_item.setKeyEquivalentModifierMask(quit_mods);
    }
    app_menu.addItem(&quit_item);

    app_menu_item.setSubmenu(Some(&app_menu));
    main_menu.addItem(&app_menu_item);

    // ── File Menu ──────────────────────────────────────────────────
    let file_menu_item = NSMenuItem::new(mtm);
    let file_menu = NSMenu::new(mtm);
    file_menu.setTitle(&NSString::from_str(t("menu.macos.file")));

    // New Window — key equivalent 는 KeybindingSettings.new_window 에서.
    let new_window_item = NSMenuItem::new(mtm);
    new_window_item.setTitle(&NSString::from_str(t("menu.macos.new_window")));
    let (nw_key, nw_mods) = keybindings
        .new_window
        .first()
        .map(|b| binding_to_nsmenu_key(b))
        .unwrap_or_else(|| (NSString::from_str(""), NSEventModifierFlags::empty()));
    new_window_item.setKeyEquivalent(&nw_key);
    // SAFETY: 위 quit_item 와 동일 — main thread + delegate 수명 보장.
    #[allow(clippy::multiple_unsafe_ops_per_block)]
    unsafe {
        new_window_item.setAction(Some(sel!(tastyNewWindow:)));
        new_window_item.setTarget(Some(&*delegate.cast()));
        new_window_item.setKeyEquivalentModifierMask(nw_mods);
    }
    file_menu.addItem(&new_window_item);

    file_menu_item.setSubmenu(Some(&file_menu));
    main_menu.addItem(&file_menu_item);

    // ── Window Menu ────────────────────────────────────────────────
    // CSD 신호등(minimize/zoom/close) 과 일관된 표준 selector. key equivalent 는
    // KeybindingSettings 에서 (없으면 빈 값).
    let window_menu_item = NSMenuItem::new(mtm);
    let window_menu = NSMenu::new(mtm);
    window_menu.setTitle(&NSString::from_str(t("menu.macos.window")));

    window_menu.addItem(&make_keybound_std_item(
        mtm,
        t("menu.macos.minimize"),
        sel!(performMiniaturize:),
        keybindings.minimize_window.first().map(|s| s.as_str()),
    ));
    window_menu.addItem(&make_keybound_std_item(
        mtm,
        t("menu.macos.zoom"),
        sel!(performZoom:),
        keybindings.maximize_window.first().map(|s| s.as_str()),
    ));
    window_menu.addItem(&NSMenuItem::separatorItem(mtm));
    window_menu.addItem(&make_keybound_std_item(
        mtm,
        t("menu.macos.close_window"),
        sel!(performClose:),
        keybindings.close_window.first().map(|s| s.as_str()),
    ));

    window_menu_item.setSubmenu(Some(&window_menu));
    main_menu.addItem(&window_menu_item);

    app.setMainMenu(Some(&main_menu));
}

/// binding 문자열 (e.g. `"alt+shift+n"`) 을 NSMenuItem 의 key equivalent + modifier mask 로 변환.
///
/// `docs/design/policies/key-mapping.md` 의 macOS 매핑:
/// - `ctrl` → Control, `shift` → Shift, `option` → Option
/// - `alt` → **Cmd** (위치 기반 추상화: macOS 의 ⌘ 는 Windows 의 Alt 와 같은 손가락 위치)
///
/// 빈 문자열 / prefix 만 있는 경우 key = `""` → 메뉴 항목에 단축키 미표시.
fn binding_to_nsmenu_key(
    binding: &str,
) -> (Retained<NSString>, objc2_app_kit::NSEventModifierFlags) {
    use objc2_app_kit::NSEventModifierFlags;
    let mut mods = NSEventModifierFlags::empty();
    let mut rest = binding;
    loop {
        let lower = rest.to_ascii_lowercase();
        if lower.starts_with("ctrl+") {
            mods |= NSEventModifierFlags::Control;
            rest = &rest[5..];
        } else if lower.starts_with("shift+") {
            mods |= NSEventModifierFlags::Shift;
            rest = &rest[6..];
        } else if lower.starts_with("alt+") {
            mods |= NSEventModifierFlags::Command;
            rest = &rest[4..];
        } else if lower.starts_with("option+") {
            mods |= NSEventModifierFlags::Option;
            rest = &rest[7..];
        } else {
            break;
        }
    }
    let key = if rest.is_empty() {
        NSString::from_str("")
    } else {
        NSString::from_str(&rest.to_ascii_lowercase())
    };
    (key, mods)
}

/// 단축키 없는 표준 NSResponder 메뉴 항목. target=nil로 responder chain에 전달한다.
/// title은 호출자가 번역해 전달한다. 단축키가 필요하면 설정 바인딩을 받는 별도 함수를 쓴다.
fn make_std_item(mtm: MainThreadMarker, title: &str, selector: Sel) -> Retained<NSMenuItem> {
    let item = NSMenuItem::new(mtm);
    item.setTitle(&NSString::from_str(title));
    item.setKeyEquivalent(&NSString::from_str(""));
    // SAFETY: main thread (mtm). setKeyEquivalentModifierMask / setAction 은 AppKit main-thread-only.
    // target 미설정 = nil = first responder chain (표준 selector 용).
    #[allow(clippy::multiple_unsafe_ops_per_block)]
    unsafe {
        item.setKeyEquivalentModifierMask(objc2_app_kit::NSEventModifierFlags::empty());
        item.setAction(Some(selector));
    }
    item
}

/// 설정 바인딩의 첫 단축키를 쓰는 표준 NSResponder 메뉴 항목.
/// target=nil이며 바인딩이 없거나 비었으면 단축키도 비운다.
fn make_keybound_std_item(
    mtm: MainThreadMarker,
    title: &str,
    selector: Sel,
    binding: Option<&str>,
) -> Retained<NSMenuItem> {
    use objc2_app_kit::NSEventModifierFlags;
    let item = NSMenuItem::new(mtm);
    item.setTitle(&NSString::from_str(title));
    let (key, mods) = binding
        .map(binding_to_nsmenu_key)
        .unwrap_or_else(|| (NSString::from_str(""), NSEventModifierFlags::empty()));
    item.setKeyEquivalent(&key);
    // SAFETY: main thread (mtm). setKeyEquivalentModifierMask / setAction 은 AppKit
    // main-thread-only. target 미설정 = nil = first responder chain (표준 selector 용).
    #[allow(clippy::multiple_unsafe_ops_per_block)]
    unsafe {
        item.setKeyEquivalentModifierMask(mods);
        item.setAction(Some(selector));
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

#[cfg(test)]
mod tests {
    use super::*;
    use objc2_app_kit::NSEventModifierFlags;

    #[test]
    fn binding_to_nsmenu_key_empty_returns_empty_key_and_empty_mods() {
        let (key, mods) = binding_to_nsmenu_key("");
        assert_eq!(key.to_string(), "");
        assert_eq!(mods, NSEventModifierFlags::empty());
    }

    #[test]
    fn binding_to_nsmenu_key_prefix_only_returns_empty_key() {
        let (key, mods) = binding_to_nsmenu_key("alt+");
        assert_eq!(key.to_string(), "");
        assert_eq!(mods, NSEventModifierFlags::Command);
    }

    #[test]
    fn binding_to_nsmenu_key_alt_maps_to_command() {
        let (key, mods) = binding_to_nsmenu_key("alt+w");
        assert_eq!(key.to_string(), "w");
        assert_eq!(mods, NSEventModifierFlags::Command);
    }

    #[test]
    fn binding_to_nsmenu_key_combo_aggregates_modifiers() {
        let (key, mods) = binding_to_nsmenu_key("alt+shift+n");
        assert_eq!(key.to_string(), "n");
        assert_eq!(
            mods,
            NSEventModifierFlags::Command | NSEventModifierFlags::Shift
        );
    }

    #[test]
    fn binding_to_nsmenu_key_lowercases_alpha_key() {
        let (key, _) = binding_to_nsmenu_key("alt+W");
        assert_eq!(key.to_string(), "w");
    }
}
