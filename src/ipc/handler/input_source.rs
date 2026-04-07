use serde_json::json;

use crate::ipc::protocol::JsonRpcResponse;

/// Switch the macOS input source (e.g. "com.apple.keylayout.ABC" or
/// "com.apple.inputmethod.Korean.2SetKorean").
pub fn handle_switch_input_source(
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let source_id = match params.get("source_id").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'source_id' parameter"),
    };

    match switch_input_source(source_id) {
        Ok(()) => JsonRpcResponse::success(id, json!({ "switched": true, "source_id": source_id })),
        Err(e) => JsonRpcResponse::internal_error(id, e),
    }
}

/// Send a raw physical key code via CGEvent. This goes through the full
/// macOS IME pipeline (interpretKeyEvents → setMarkedText/insertText).
pub fn handle_raw_key(
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let keycode = match params.get("keycode").and_then(|v| v.as_u64()) {
        Some(k) if k <= u16::MAX as u64 => k as u16,
        _ => return JsonRpcResponse::invalid_params(id, "Missing or invalid 'keycode' (u16)"),
    };
    let direction = params
        .get("direction")
        .and_then(|v| v.as_str())
        .unwrap_or("click");

    match direction {
        "press" => post_key_event(keycode, true),
        "release" => post_key_event(keycode, false),
        "click" | _ => {
            post_key_event(keycode, true);
            std::thread::sleep(std::time::Duration::from_millis(30));
            post_key_event(keycode, false);
        }
    }

    JsonRpcResponse::success(id, json!({ "sent": true, "keycode": keycode }))
}

// ---- macOS FFI ----

use std::ffi::c_void;

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn TISCreateInputSourceList(
        properties: *const c_void,
        include_all: bool,
    ) -> *const c_void;
    fn TISSelectInputSource(source: *const c_void) -> i32;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFStringCreateWithCString(
        alloc: *const c_void,
        c_str: *const u8,
        encoding: u32,
    ) -> *const c_void;
    fn CFDictionaryCreate(
        alloc: *const c_void,
        keys: *const *const c_void,
        values: *const *const c_void,
        count: isize,
        key_callbacks: *const c_void,
        value_callbacks: *const c_void,
    ) -> *const c_void;
    fn CFArrayGetCount(array: *const c_void) -> isize;
    fn CFArrayGetValueAtIndex(array: *const c_void, idx: isize) -> *const c_void;
    fn CFRelease(cf: *const c_void);
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGEventSourceCreate(state: i32) -> *const c_void;
    fn CGEventCreateKeyboardEvent(
        source: *const c_void,
        keycode: u16,
        key_down: bool,
    ) -> *const c_void;
    fn CGEventPost(tap: u32, event: *const c_void);
}

const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;

fn cf_string(s: &str) -> *const c_void {
    let c_str = std::ffi::CString::new(s).unwrap();
    unsafe { CFStringCreateWithCString(std::ptr::null(), c_str.as_ptr() as _, K_CF_STRING_ENCODING_UTF8) }
}

fn switch_input_source(source_id: &str) -> Result<(), String> {
    unsafe {
        let key = cf_string("TISPropertyInputSourceID");
        let val = cf_string(source_id);
        let keys = [key];
        let vals = [val];
        let filter = CFDictionaryCreate(
            std::ptr::null(),
            keys.as_ptr(),
            vals.as_ptr(),
            1,
            std::ptr::null(),
            std::ptr::null(),
        );
        let list = TISCreateInputSourceList(filter, false);
        let count = CFArrayGetCount(list);
        if count > 0 {
            let src = CFArrayGetValueAtIndex(list, 0);
            let result = TISSelectInputSource(src);
            CFRelease(list);
            CFRelease(filter);
            if result == 0 {
                Ok(())
            } else {
                Err(format!("TISSelectInputSource failed: {}", result))
            }
        } else {
            CFRelease(list);
            CFRelease(filter);
            Err(format!("Input source '{}' not found", source_id))
        }
    }
}

fn post_key_event(keycode: u16, key_down: bool) {
    unsafe {
        // kCGEventSourceStateCombinedSessionState = 0
        let source = CGEventSourceCreate(0);
        let event = CGEventCreateKeyboardEvent(source, keycode, key_down);
        // kCGAnnotatedSessionEventTap = 2
        CGEventPost(2, event);
    }
}
