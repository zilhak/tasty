//! Windows 클립보드의 `CF_HDROP` 과 `Preferred DropEffect`. Explorer 가 쓰는 형식과 같다.
//! 스크린샷은 `CF_UNICODETEXT`(경로)·`PNG`·`CF_DIB` 를 한 번 연 클립보드에 함께 올린다.
//!
//! 클립보드는 호출 스레드의 메시지 전용 창을 소유자로 열어 연다. 소유 창 없이 연 클립보드에서
//! `EmptyClipboard` 뒤 `SetClipboardData` 는 실패한다고 문서화돼 있다.

use std::cell::Cell;

use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL, HWND};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
    RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{
    GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, HWND_MESSAGE, WINDOW_EX_STYLE, WINDOW_STYLE,
};
use windows::core::w;

use super::format;
use super::{OsClip, OsFiles};

/// 표준 형식 `CF_HDROP`.
const CF_HDROP: u32 = 15;
/// 표준 형식 `CF_UNICODETEXT`·`CF_DIB`.
const CF_UNICODETEXT: u32 = 13;
const CF_DIB: u32 = 8;

/// 다른 프로그램이 클립보드를 잡고 있을 때 다시 여는 횟수와 간격.
const OPEN_ATTEMPTS: u32 = 10;
const OPEN_RETRY: std::time::Duration = std::time::Duration::from_millis(10);

fn drop_effect_format() -> Result<u32, String> {
    // SAFETY: 상수 와이드 문자열을 넘긴다.
    match unsafe { RegisterClipboardFormatW(w!("Preferred DropEffect")) } {
        0 => Err("RegisterClipboardFormatW failed".into()),
        id => Ok(id),
    }
}

thread_local! {
    /// 이 스레드의 클립보드 소유 창. 처음 열 때 만들고 스레드가 끝날 때까지 둔다.
    static OWNER: Cell<Option<HWND>> = const { Cell::new(None) };
}

/// 호출 스레드의 메시지 전용 창. 지연 렌더링을 쓰지 않아 렌더링 요청은 오지 않는다. 다만 다른
/// 프로그램이 `EmptyClipboard` 하면 소유 창에 `WM_DESTROYCLIPBOARD` 가 가므로, 메시지를 처리하는
/// 스레드(이벤트 루프)에서만 게시한다. 메시지를 처리하지 않는 worker 스레드가 소유하면 그 프로그램의
/// 복사가 늦어지거나 멈출 수 있다.
fn owner_window() -> Result<HWND, String> {
    if let Some(hwnd) = OWNER.get() {
        return Ok(hwnd);
    }
    // SAFETY: 시스템 클래스 STATIC 의 보이지 않는 메시지 전용 창을 만든다. 포인터 인자는 없다.
    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("STATIC"),
            w!(""),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            None,
            None,
        )
    }
    .map_err(|e| format!("CreateWindowExW failed: {e}"))?;
    OWNER.set(Some(hwnd));
    Ok(hwnd)
}

/// 열린 클립보드. 놓을 때 닫는다.
struct Opened;

impl Opened {
    fn new() -> Result<Self, String> {
        let owner = owner_window()?;
        let mut last = String::new();
        for _ in 0..OPEN_ATTEMPTS {
            // SAFETY: 이 스레드가 만든 창을 소유자로 연다. 같은 스레드에서 Drop 이 닫는다.
            match unsafe { OpenClipboard(Some(owner)) } {
                Ok(()) => return Ok(Self),
                Err(e) => last = e.to_string(),
            }
            std::thread::sleep(OPEN_RETRY);
        }
        Err(format!("OpenClipboard failed: {last}"))
    }
}

impl Drop for Opened {
    fn drop(&mut self) {
        // SAFETY: new 가 연 클립보드를 닫는다.
        if let Err(e) = unsafe { CloseClipboard() } {
            tracing::warn!("CloseClipboard failed: {e}");
        }
    }
}

/// 바이트를 이동 가능한 전역 메모리에 복사해 클립보드에 넘긴다. 넘긴 뒤에는 시스템이 소유한다.
fn set(format: u32, bytes: &[u8]) -> Result<(), String> {
    // SAFETY: 크기만 넘기는 할당이다.
    let mem: HGLOBAL =
        unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes.len()) }.map_err(|e| e.to_string())?;
    // SAFETY: 위에서 할당한 핸들이다.
    let ptr = unsafe { GlobalLock(mem) };
    if ptr.is_null() {
        free(mem);
        return Err("GlobalLock failed".into());
    }
    // SAFETY: 잠근 메모리는 bytes.len() 바이트이고 두 영역은 겹치지 않는다.
    unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr.cast::<u8>(), bytes.len()) };
    unlock(mem);
    // SAFETY: 클립보드가 열려 있고, 성공하면 메모리 소유가 시스템으로 넘어간다.
    if let Err(e) = unsafe { SetClipboardData(format, Some(HANDLE(mem.0))) } {
        free(mem);
        return Err(format!("SetClipboardData failed: {e}"));
    }
    Ok(())
}

/// 잠금을 푼다. 잠금 수가 0 이 되면 코드 0(NO_ERROR)의 Err 를 돌려주며, 그 경우는 실패가 아니다.
fn unlock(mem: HGLOBAL) {
    // SAFETY: 이 모듈이 잠근 핸들만 넘긴다.
    if let Err(e) = unsafe { GlobalUnlock(mem) }
        && e.code().0 != 0
    {
        tracing::debug!("GlobalUnlock: {e}");
    }
}

fn free(mem: HGLOBAL) {
    // SAFETY: 클립보드에 넘기지 못한 메모리만 해제한다.
    if let Err(e) = unsafe { GlobalFree(Some(mem)) } {
        tracing::warn!("GlobalFree failed: {e}");
    }
}

/// 클립보드의 형식 하나를 바이트로 읽는다. 없으면 None. 클립보드가 열려 있어야 한다.
fn get(format: u32) -> Option<Vec<u8>> {
    // SAFETY: 형식 번호만 넘긴다.
    unsafe { IsClipboardFormatAvailable(format) }.ok()?;
    // SAFETY: 클립보드가 열려 있다. 돌려받은 핸들은 클립보드가 소유하므로 해제하지 않는다.
    let handle = unsafe { GetClipboardData(format) }.ok()?;
    let mem = HGLOBAL(handle.0);
    // SAFETY: 클립보드가 준 전역 메모리 핸들이다.
    let ptr = unsafe { GlobalLock(mem) };
    if ptr.is_null() {
        return None;
    }
    // SAFETY: 잠근 메모리의 크기다.
    let len = unsafe { GlobalSize(mem) };
    // SAFETY: 잠근 동안 len 바이트를 읽을 수 있다.
    let bytes = unsafe { std::slice::from_raw_parts(ptr.cast::<u8>(), len) }.to_vec();
    unlock(mem);
    Some(bytes)
}

pub(super) fn write(files: &OsFiles) -> Result<(), String> {
    let effect_format = drop_effect_format()?;
    let effect = if files.cut {
        format::DROPEFFECT_MOVE
    } else {
        format::DROPEFFECT_COPY
    };
    let _open = Opened::new()?;
    // SAFETY: 클립보드가 열려 있다.
    unsafe { EmptyClipboard() }.map_err(|e| format!("EmptyClipboard failed: {e}"))?;
    set(CF_HDROP, &format::dropfiles(&files.paths))?;
    set(effect_format, &effect.to_le_bytes())
}

/// 경로 텍스트와 이미지를 한 번 연 클립보드에 함께 올린다. 형식 하나라도 실패하면 Err 이며, 그때
/// 클립보드에 일부 형식이 남을 수 있어 호출자가 텍스트만 다시 쓴다.
pub(crate) fn write_text_and_image(
    text: &str,
    image: &crate::ports::clipboard::ClipboardImage,
) -> Result<(), String> {
    let dib = format::dib(image.width, image.height, &image.rgba)
        .ok_or("image pixels do not match its size")?;
    // SAFETY: 상수 와이드 문자열을 넘긴다.
    let png_format = match unsafe { RegisterClipboardFormatW(w!("PNG")) } {
        0 => return Err("RegisterClipboardFormatW(PNG) failed".into()),
        id => id,
    };
    let _open = Opened::new()?;
    // SAFETY: 클립보드가 열려 있다.
    unsafe { EmptyClipboard() }.map_err(|e| format!("EmptyClipboard failed: {e}"))?;
    set(CF_UNICODETEXT, &format::unicode_text(text))?;
    set(png_format, &image.png)?;
    set(CF_DIB, &dib)
}

pub(super) fn read() -> Result<OsClip, String> {
    let effect_format = drop_effect_format()?;
    let _open = Opened::new()?;
    let Some(bytes) = get(CF_HDROP) else {
        return Ok(OsClip::Other);
    };
    let Some(paths) = format::parse_dropfiles(&bytes) else {
        return Ok(OsClip::Other);
    };
    let cut = get(effect_format)
        .and_then(|b| Some(u32::from_le_bytes(b.get(..4)?.try_into().ok()?)))
        .is_some_and(format::drop_effect_is_cut);
    Ok(OsClip::from_paths(paths, cut))
}

pub(super) fn clear() -> Result<(), String> {
    let _open = Opened::new()?;
    // SAFETY: 클립보드가 열려 있다.
    unsafe { EmptyClipboard() }.map_err(|e| format!("EmptyClipboard failed: {e}"))
}
