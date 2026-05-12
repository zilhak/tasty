//! Windows file clipboard using CF_HDROP + Preferred DropEffect.
//!
//! Sets two clipboard formats simultaneously:
//! 1. CF_HDROP — DROPFILES struct + UTF-16 file path array
//! 2. "Preferred DropEffect" — DWORD indicating copy (1) or move (2)
//!
//! 본 모듈의 unsafe 블록은 모두 Win32 클립보드 API의 표준 호출 패턴을 따른다:
//! `OpenClipboard` → `SetClipboardData`/`GetClipboardData` → `CloseClipboard` 시퀀스를
//! 단일 함수 안에서 완결한다. 다른 스레드에서 동시에 클립보드를 잡으면 OS가
//! `OpenClipboard` 단계에서 실패시키므로 race는 OS가 보장한다.

use std::ptr;

use windows::Win32::Foundation::{HANDLE, HGLOBAL};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
    RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{
    GLOBAL_ALLOC_FLAGS, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock,
};
use windows::Win32::UI::Shell::{DROPFILES, DragQueryFileW, HDROP};
use windows::core::w;

use super::FileClipboardOp;

/// CF_HDROP clipboard format ID (predefined = 15).
const CF_HDROP: u32 = 15;

/// GMEM_MOVEABLE | GMEM_ZEROINIT
const GHND: GLOBAL_ALLOC_FLAGS = GLOBAL_ALLOC_FLAGS(0x0042);

/// Copy or cut file paths to the OS clipboard using CF_HDROP.
pub fn set_file_clipboard(paths: &[&str], op: FileClipboardOp) -> Result<(), String> {
    if paths.is_empty() {
        return Err("No paths provided".to_string());
    }

    // SAFETY: RegisterClipboardFormatW는 정적 문자열로 호출 가능하며 thread-safe.
    // w!() 매크로가 만든 PCWSTR은 'static lifetime 보장.
    let drop_effect_fmt = unsafe { RegisterClipboardFormatW(w!("Preferred DropEffect")) };
    if drop_effect_fmt == 0 {
        return Err("Failed to register Preferred DropEffect format".to_string());
    }

    // Build the DROPFILES + UTF-16 paths buffer
    let hdrop_data = build_dropfiles_data(paths)?;

    // Build the drop effect DWORD
    let effect_value: u32 = match op {
        FileClipboardOp::Copy => 1, // DROPEFFECT_COPY
        FileClipboardOp::Cut => 2,  // DROPEFFECT_MOVE
    };

    // SAFETY: 본 블록은 표준 Win32 클립보드 시퀀스를 한 함수 안에서 완결한다:
    // OpenClipboard → EmptyClipboard → SetClipboardData* → CloseClipboard.
    // 모든 분기에서 CloseClipboard가 호출되도록 명시 처리. SetClipboardData가
    // 성공하면 HGLOBAL 소유권이 OS로 이전되므로 누수 없음.
    unsafe {
        OpenClipboard(None).map_err(|e| format!("OpenClipboard failed: {e}"))?;

        if let Err(e) = EmptyClipboard() {
            let _ = CloseClipboard();
            return Err(format!("EmptyClipboard failed: {e}"));
        }

        let hdrop_hmem = alloc_global_data(&hdrop_data)?;
        let hdrop_handle = HANDLE(hdrop_hmem.0);
        if let Err(_e) = SetClipboardData(CF_HDROP, Some(hdrop_handle)) {
            let _ = CloseClipboard();
            return Err("SetClipboardData(CF_HDROP) failed".to_string());
        }

        let effect_bytes = effect_value.to_le_bytes();
        let effect_hmem = alloc_global_data(&effect_bytes)?;
        let effect_handle = HANDLE(effect_hmem.0);
        if let Err(_e) = SetClipboardData(drop_effect_fmt, Some(effect_handle)) {
            let _ = CloseClipboard();
            return Err("SetClipboardData(DropEffect) failed".to_string());
        }

        CloseClipboard().map_err(|e| format!("CloseClipboard failed: {e}"))?;
    }

    Ok(())
}

/// Read file paths from the OS clipboard (CF_HDROP).
/// Returns None if clipboard doesn't contain file URLs.
pub fn get_file_clipboard() -> Result<Option<(Vec<String>, FileClipboardOp)>, String> {
    // SAFETY: IsClipboardFormatAvailable은 lock 없이 호출 가능. 이후 OpenClipboard
    // 성공 후 inner를 호출하고 반드시 CloseClipboard로 정리한다. inner 자체가
    // unsafe fn이므로 호출자(여기)가 클립보드 open 상태를 보장한다.
    unsafe {
        if IsClipboardFormatAvailable(CF_HDROP).is_err() {
            return Ok(None);
        }

        OpenClipboard(None).map_err(|e| format!("OpenClipboard failed: {e}"))?;

        let result = get_file_clipboard_inner();

        let _ = CloseClipboard();

        result
    }
}

unsafe fn get_file_clipboard_inner() -> Result<Option<(Vec<String>, FileClipboardOp)>, String> {
    // SAFETY: 호출자가 OpenClipboard 성공 후에만 본 함수를 호출하도록 시그니처를
    // unsafe fn으로 강제. GetClipboardData가 반환하는 HANDLE은 close 전까지 valid.
    let handle = match unsafe { GetClipboardData(CF_HDROP) } {
        Ok(h) => h,
        Err(_) => return Ok(None),
    };
    let hdrop = HDROP(handle.0);

    // SAFETY: DragQueryFileW에 0xFFFFFFFF 전달 시 file count 반환 (Win32 문서).
    // hdrop은 위에서 GetClipboardData로 얻어 클립보드 close 전까지 valid.
    let count = unsafe { DragQueryFileW(hdrop, 0xFFFFFFFF, None) };
    if count == 0 {
        return Ok(None);
    }

    let mut paths = Vec::with_capacity(count as usize);

    for i in 0..count {
        // SAFETY: buf=None일 때 DragQueryFileW는 null 제외 char 수를 반환 (Win32 문서).
        let len = unsafe { DragQueryFileW(hdrop, i, None) };
        if len == 0 {
            continue;
        }

        let mut buf = vec![0u16; (len + 1) as usize];
        // SAFETY: buf는 len+1 만큼 사전 할당 — null 종단까지 포함해 충분.
        unsafe { DragQueryFileW(hdrop, i, Some(&mut buf)) };

        let path = String::from_utf16_lossy(&buf[..len as usize]);
        if !path.is_empty() {
            paths.push(path);
        }
    }

    if paths.is_empty() {
        return Ok(None);
    }

    // SAFETY: read_drop_effect도 클립보드 open 상태를 요구하는 unsafe fn — 호출자(여기)가
    // 그 invariant를 충족한 상태.
    let op = unsafe { read_drop_effect() }.unwrap_or(FileClipboardOp::Copy);

    Ok(Some((paths, op)))
}

unsafe fn read_drop_effect() -> Option<FileClipboardOp> {
    // SAFETY: 호출자가 클립보드 open 상태에서 호출 (unsafe fn 시그니처로 명시).
    let drop_effect_fmt = unsafe { RegisterClipboardFormatW(w!("Preferred DropEffect")) };
    if drop_effect_fmt == 0 {
        return None;
    }

    // SAFETY: 클립보드 open 상태에서만 GetClipboardData 호출 안전 (caller invariant).
    let handle = unsafe { GetClipboardData(drop_effect_fmt) }.ok()?;
    let hglobal = HGLOBAL(handle.0);
    // SAFETY: hglobal은 GetClipboardData 반환값으로 close 전까지 valid. GlobalLock은
    // 동일 thread에서 호출되므로 lock count를 올린다.
    let ptr = unsafe { GlobalLock(hglobal) };
    if ptr.is_null() {
        return None;
    }

    // SAFETY: hglobal이 valid면 GlobalSize는 0 또는 실제 크기 반환.
    let size = unsafe { GlobalSize(hglobal) };
    let op = if size >= 4 {
        // SAFETY: size >= 4 확인 후 u32 한 워드 읽기. ptr은 lock된 HGLOBAL 매핑.
        // Preferred DropEffect는 정의상 little-endian u32(1=copy, 2=move) 1워드.
        let value = unsafe { *(ptr as *const u32) };
        if value == 2 {
            FileClipboardOp::Cut // DROPEFFECT_MOVE
        } else {
            FileClipboardOp::Copy // DROPEFFECT_COPY (default)
        }
    } else {
        FileClipboardOp::Copy
    };

    // SAFETY: 위 GlobalLock의 짝. lock count를 다시 내린다.
    let _ = unsafe { GlobalUnlock(hglobal) };
    Some(op)
}

/// Build DROPFILES struct + UTF-16 null-terminated paths + double null terminator.
fn build_dropfiles_data(paths: &[&str]) -> Result<Vec<u8>, String> {
    let dropfiles_size = std::mem::size_of::<DROPFILES>();

    // Convert paths to UTF-16 with null terminators
    let mut wide_data: Vec<u16> = Vec::new();
    for path in paths {
        let wide: Vec<u16> = path.encode_utf16().collect();
        wide_data.extend_from_slice(&wide);
        wide_data.push(0); // null terminator for each path
    }
    wide_data.push(0); // double null terminator

    let wide_bytes_len = wide_data.len() * 2;
    let total_size = dropfiles_size + wide_bytes_len;

    let mut data = vec![0u8; total_size];

    // Fill DROPFILES header
    let dropfiles = data.as_mut_ptr() as *mut DROPFILES;
    // SAFETY: data는 total_size = sizeof(DROPFILES) + wide_bytes_len 만큼 할당.
    // dropfiles 포인터는 data 시작 = 사이즈 충분, 정렬은 Vec<u8>이라 1바이트지만
    // DROPFILES는 packed가 아니어도 헤더 필드 단위 write는 unaligned-safe 컴파일러
    // 처리(memcpy semantic)로 안전. zeroed 초기화 후 pFiles/fWide만 set.
    unsafe {
        (*dropfiles).pFiles = dropfiles_size as u32;
        (*dropfiles).fWide = true.into();
        // pt and fNC are zero-initialized by default
    }

    // Copy UTF-16 path data after the header
    let path_offset = dropfiles_size;
    // SAFETY: wide_data의 ptr은 element=u16 * 길이 wide_data.len(). 이를 u8 슬라이스로
    // wide_bytes_len = len*2 만큼 노출. 메모리는 wide_data가 살아있는 동안 유효.
    let wide_bytes =
        unsafe { std::slice::from_raw_parts(wide_data.as_ptr() as *const u8, wide_bytes_len) };
    data[path_offset..path_offset + wide_bytes_len].copy_from_slice(wide_bytes);

    Ok(data)
}

/// Allocate global memory and copy data into it.
/// Returns the HGLOBAL handle (ownership transferred to clipboard on SetClipboardData).
unsafe fn alloc_global_data(data: &[u8]) -> Result<HGLOBAL, String> {
    // SAFETY: GlobalAlloc(GHND, len)은 GMEM_MOVEABLE|GMEM_ZEROINIT — 표준 사용.
    let hmem =
        unsafe { GlobalAlloc(GHND, data.len()) }.map_err(|e| format!("GlobalAlloc failed: {e}"))?;

    // SAFETY: hmem은 방금 할당 → 같은 thread에서 lock 가능.
    let ptr = unsafe { GlobalLock(hmem) };
    if ptr.is_null() {
        return Err("GlobalLock returned null".to_string());
    }

    // SAFETY: ptr은 lock된 hmem의 매핑, data.len() 만큼 zeroed로 사전 할당. src/dst
    // 영역이 겹치지 않음 (별도 alloc + 별도 slice).
    unsafe { ptr::copy_nonoverlapping(data.as_ptr(), ptr as *mut u8, data.len()) };
    // SAFETY: 위 GlobalLock의 짝.
    let _ = unsafe { GlobalUnlock(hmem) };

    Ok(hmem)
}
