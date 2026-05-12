//! macOS file clipboard using NSPasteboard + writeObjects (NSURL).
//!
//! Uses `writeObjects:` with NSURL array so Finder can paste multiple files.

use objc2::runtime::ProtocolObject;
use objc2_app_kit::{NSPasteboard, NSPasteboardWriting};
use objc2_foundation::{NSArray, NSString, NSURL};

use super::FileClipboardOp;

/// Copy or cut file paths to the OS clipboard.
/// Uses writeObjects: with NSURL array — Finder-compatible for multiple files.
pub fn set_file_clipboard(paths: &[&str], _op: FileClipboardOp) -> Result<(), String> {
    let pasteboard = NSPasteboard::generalPasteboard();
    pasteboard.clearContents();

    let urls: Vec<_> = paths
        .iter()
        .map(|p| NSURL::fileURLWithPath(&NSString::from_str(p)))
        .collect();

    // Convert Vec<Retained<NSURL>> → Vec<&ProtocolObject<dyn NSPasteboardWriting>>
    let protocol_objects: Vec<&ProtocolObject<dyn NSPasteboardWriting>> = urls
        .iter()
        .map(|url| ProtocolObject::from_ref::<NSURL>(url.as_ref()))
        .collect();

    let array = NSArray::from_slice(&protocol_objects);
    pasteboard.writeObjects(&array);

    Ok(())
}

/// Read file paths from the OS clipboard.
/// Returns None if clipboard doesn't contain file URLs.
/// Reads all pasteboard items to support multiple files (e.g. from Finder).
pub fn get_file_clipboard() -> Result<Option<(Vec<String>, FileClipboardOp)>, String> {
    let pasteboard = NSPasteboard::generalPasteboard();
    // SAFETY: NSPasteboardTypeFileURL은 AppKit이 노출하는 static NSString 상수 — 'static lifetime.
    // 단순 reference 캐스팅이므로 main thread 제약 없음.
    let file_url_type: &NSString = unsafe { objc2_app_kit::NSPasteboardTypeFileURL };

    let mut paths = Vec::new();

    // Iterate pasteboard items to read all file URLs
    if let Some(items) = pasteboard.pasteboardItems() {
        for item in items.iter() {
            if let Some(s) = item.stringForType(file_url_type) {
                let url_str = s.to_string();
                let path = if let Some(p) = url_str.strip_prefix("file://") {
                    percent_decode(p)
                } else {
                    url_str
                };
                if !path.is_empty() {
                    paths.push(path);
                }
            }
        }
    }

    if paths.is_empty() {
        Ok(None)
    } else {
        Ok(Some((paths, FileClipboardOp::Copy)))
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
