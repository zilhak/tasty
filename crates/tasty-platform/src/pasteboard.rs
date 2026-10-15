//! macOS 일반 pasteboard 에 경로 텍스트와 PNG 를 함께 올린다. 한 번 비운 뒤 두 형식을 같은 항목에
//! 쓰므로 뒤의 쓰기가 앞의 형식을 지우지 않는다. 다른 OS 의 같은 기능은 본체의 클립보드 모듈이 맡는다.

use objc2_app_kit::{NSPasteboard, NSPasteboardTypePNG, NSPasteboardTypeString};
use objc2_foundation::{NSData, NSString};

/// 실패하면 pasteboard 에 일부 형식만 남을 수 있다. 호출자는 텍스트만 다시 쓴다.
pub fn write_text_and_png(text: &str, png: &[u8]) -> Result<(), String> {
    let pasteboard = NSPasteboard::generalPasteboard();
    pasteboard.clearContents();
    // SAFETY: AppKit 이 내보내는 상수 문자열이다.
    let string_type = unsafe { NSPasteboardTypeString };
    // SAFETY: 위와 같다.
    let png_type = unsafe { NSPasteboardTypePNG };
    if !pasteboard.setString_forType(&NSString::from_str(text), string_type) {
        return Err("NSPasteboard setString:forType: failed".into());
    }
    if !pasteboard.setData_forType(Some(&NSData::with_bytes(png)), png_type) {
        return Err("NSPasteboard setData:forType: (PNG) failed".into());
    }
    Ok(())
}
