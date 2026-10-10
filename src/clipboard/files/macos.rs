//! macOS 일반 pasteboard 의 파일 URL.
//! Finder 의 복사에는 잘라내기 표지가 없어(옮기기는 붙여넣는 쪽의 `⌥⌘V` 가 정한다) 늘 복사로 읽고,
//! Tasty 의 잘라내기도 표지 없이 올린다. 쓰기·비우기는 arboard 를 쓰고, 읽기는 파일이 아닌 URL 이
//! 섞였는지 알아야 해서 pasteboard 항목을 직접 읽는다(arboard 의 파일 목록은 파일 URL 만 골라 준다).

use objc2_app_kit::{NSPasteboard, NSPasteboardTypeFileURL, NSPasteboardTypeURL};

use super::{OsClip, OsFiles, format};

fn clipboard() -> Result<arboard::Clipboard, String> {
    arboard::Clipboard::new().map_err(|e| e.to_string())
}

pub(super) fn write(files: &OsFiles) -> Result<(), String> {
    clipboard()?
        .set()
        .file_list(&files.paths)
        .map_err(|e| e.to_string())
}

/// 항목마다 파일 URL 을, 없으면 일반 URL 을 모은다. URL 이 없는 항목(글자 등)은 건너뛴다.
pub(super) fn read() -> Result<OsClip, String> {
    let pasteboard = NSPasteboard::generalPasteboard();
    let Some(items) = pasteboard.pasteboardItems() else {
        return Ok(OsClip::Other);
    };
    // SAFETY: AppKit 이 내보내는 상수 문자열이다.
    let file_url = unsafe { NSPasteboardTypeFileURL };
    // SAFETY: 위와 같다.
    let url = unsafe { NSPasteboardTypeURL };
    let mut urls = Vec::new();
    for i in 0..items.count() {
        let item = items.objectAtIndex(i);
        if let Some(s) = item
            .stringForType(file_url)
            .or_else(|| item.stringForType(url))
        {
            urls.push(s.to_string());
        }
    }
    Ok(format::parse_pasteboard_urls(&urls))
}

pub(super) fn clear() -> Result<(), String> {
    clipboard()?.clear().map_err(|e| e.to_string())
}
