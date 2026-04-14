//! macOS file clipboard using NSPasteboard + NSPasteboardTypeFileURL.

use objc2_app_kit::NSPasteboard;
use objc2_foundation::{NSString, NSURL};

use super::FileClipboardOp;

/// Copy or cut file paths to the OS clipboard.
/// Uses NSPasteboardTypeFileURL so Finder can paste them.
pub fn set_file_clipboard(paths: &[&str], _op: FileClipboardOp) -> Result<(), String> {
    unsafe {
        let pasteboard = NSPasteboard::generalPasteboard();
        pasteboard.clearContents();

        // Write file URLs as strings in NSPasteboardTypeFileURL format
        for path in paths {
            let url = NSURL::fileURLWithPath(&NSString::from_str(path));
            let url_string = url.absoluteString()
                .ok_or("Failed to get URL string")?;
            let file_url_type: &NSString = objc2_app_kit::NSPasteboardTypeFileURL;
            pasteboard.setString_forType(&url_string, file_url_type);
        }
        Ok(())
    }
}

/// Read file paths from the OS clipboard.
/// Returns None if clipboard doesn't contain file URLs.
pub fn get_file_clipboard() -> Result<Option<(Vec<String>, FileClipboardOp)>, String> {
    unsafe {
        let pasteboard = NSPasteboard::generalPasteboard();
        let file_url_type: &NSString = objc2_app_kit::NSPasteboardTypeFileURL;
        let string = pasteboard.stringForType(file_url_type);
        if let Some(s) = string {
            let url_str = s.to_string();
            let path = if let Some(p) = url_str.strip_prefix("file://") {
                percent_decode(p)
            } else {
                url_str
            };
            if !path.is_empty() {
                return Ok(Some((vec![path], FileClipboardOp::Copy)));
            }
        }
        Ok(None)
    }
}

fn percent_decode(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.bytes();
    while let Some(b) = chars.next() {
        if b == b'%' {
            let hi = chars.next().and_then(|c| hex_val(c));
            let lo = chars.next().and_then(|c| hex_val(c));
            if let (Some(h), Some(l)) = (hi, lo) {
                result.push((h << 4 | l) as char);
            }
        } else {
            result.push(b as char);
        }
    }
    result
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}
