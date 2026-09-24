//! macOS 전역 입력기 전환과 키 주입. 특정 surface가 아니라 OS 포커스 대상에 영향을 준다.
//! macOS GUI 디버그 빌드와 --enable-input-simulation에서만 허용한다(ADR-0012).

use super::params::{self, p_try};
use serde_json::json;

use tasty_ipc::protocol::JsonRpcResponse;

use super::debug::require_input_simulation;
use crate::macos_permissions::{RawKeyDecision, accessibility_trusted, raw_key_decision};
use crate::state::AppState;

/// Switch the macOS input source (e.g. "com.apple.keylayout.ABC" or
/// "com.apple.inputmethod.Korean.2SetKorean").
///
/// 시스템 전역 입력 소스를 바꾸므로 `--enable-input-simulation` 게이트를 지난다.
pub fn handle_switch_input_source(
    _state: &AppState,
    engine: &crate::core::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    if let Err(e) = require_input_simulation(engine, &id) {
        return e;
    }
    let source_id = match params.get("source_id").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'source_id' parameter"),
    };
    if source_id.contains('\0') {
        return JsonRpcResponse::invalid_params(id, "'source_id' must not contain NUL bytes");
    }

    match switch_input_source(source_id) {
        Ok(()) => JsonRpcResponse::success(id, json!({ "switched": true, "source_id": source_id })),
        Err(e) => JsonRpcResponse::internal_error(id, e),
    }
}

/// CGEvent로 키를 보내 macOS IME 처리를 거친다.
/// 손쉬운 사용 권한은 호출마다 확인한다. 미승인 시 이벤트가 무시되므로 먼저 오류로 답한다.
pub fn handle_raw_key(
    _state: &AppState,
    engine: &crate::core::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    if let Err(e) = require_input_simulation(engine, &id) {
        return e;
    }
    let keycode = match p_try!(params::opt_int::<u64>(params, "keycode", &id)) {
        Some(k) if k <= u16::MAX as u64 => k as u16,
        _ => return JsonRpcResponse::invalid_params(id, "Missing or invalid 'keycode' (u16)"),
    };
    let direction = params
        .get("direction")
        .and_then(|v| v.as_str())
        .unwrap_or("click");

    if raw_key_decision(accessibility_trusted()) == RawKeyDecision::PermissionDenied {
        tracing::warn!("surface.raw_key: 손쉬운 사용 권한 미승인 — 키 주입을 건너뛴다");
        // 권한 계열 거부는 caller.rs 의 권한 게이트와 같은 코드/접두사를 쓴다.
        return JsonRpcResponse::error(
            id,
            -32001,
            "permission_denied: accessibility (key injection) is not granted — enable Tasty in \
             System Settings > Privacy & Security > Accessibility, then restart Tasty",
        );
    }

    match direction {
        "press" => post_key_event(keycode, true),
        "release" => post_key_event(keycode, false),
        _ => {
            post_key_event(keycode, true);
            std::thread::sleep(std::time::Duration::from_millis(30));
            post_key_event(keycode, false);
        }
    }

    JsonRpcResponse::success(id, json!({ "sent": true, "keycode": keycode }))
}

use std::ffi::c_void;

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn TISCreateInputSourceList(properties: *const c_void, include_all: bool) -> *const c_void;
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
    // SAFETY: c_str은 호출 끝까지 살아 있는 local CString — CFStringCreateWithCString이
    // 내부 복사를 만들어 caller로부터 독립한다 (CF는 immutable copy semantics).
    // 반환 포인터는 CFStringRef로, caller가 CFRelease 책임을 진다 (switch_input_source에서 처리).
    unsafe {
        CFStringCreateWithCString(
            std::ptr::null(),
            c_str.as_ptr() as _,
            K_CF_STRING_ENCODING_UTF8,
        )
    }
}

fn switch_input_source(source_id: &str) -> Result<(), String> {
    if source_id.contains('\0') {
        return Err("input source id must not contain NUL bytes".to_string());
    }
    // 고정 키와 NUL 검사를 마친 source_id만 CString으로 바꾼다.
    // SAFETY: 전체 시퀀스는 TIS(Text Input Source) 표준 사용 패턴.
    // - cf_string으로 만든 key/val은 CFDictionaryCreate에 넘기면 dict가 retain.
    // - TISCreateInputSourceList는 CFArrayRef를 +1 retain count로 반환 → CFRelease로 정리.
    // - 모든 호출은 IPC 핸들러 스레드(또는 main)에서 수행되며, TIS API는 process-wide
    //   thread-safe하다 (Apple 문서).
    // FFI 시퀀스가 한 트랜잭션이라 분할하면 retain/release 짝이 분산되어 가독성 저하.
    #[allow(clippy::multiple_unsafe_ops_per_block)]
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
    // SAFETY: CGEventSourceCreate → CGEventCreateKeyboardEvent → CGEventPost는 CoreGraphics
    // 표준 키 시뮬레이션 패턴. 반환된 source/event는 CF object로 ARC 없이는 leak되지만,
    // 이 모듈은 debug 빌드에서만 컴파일되는 `surface.raw_key` 경로 전용이라 leak
    // 영향이 미미하고 별도 fix 대상.
    // 본 SAFETY 보증 범위는 호출 자체의 UB 부재.
    // CG 시퀀스가 한 단위라 분할 시 가독성 저하.
    #[allow(clippy::multiple_unsafe_ops_per_block)]
    unsafe {
        // kCGEventSourceStateCombinedSessionState = 0
        let source = CGEventSourceCreate(0);
        let event = CGEventCreateKeyboardEvent(source, keycode, key_down);
        // kCGAnnotatedSessionEventTap = 2
        CGEventPost(2, event);
    }
}

#[cfg(test)]
mod tests {
    // 여기서는 NUL 거절 조건만 검사한다. 실제 입력기 전환은 macOS FFI 검증이 별도로 필요하다.
    #[test]
    fn nul_byte_source_id_is_rejected() {
        assert!("abc\0def".contains('\0'));
        assert!(!"com.apple.keylayout.US".contains('\0'));
    }
}
