//! Windows file clipboard using CF_HDROP + Preferred DropEffect.
//!
//! Sets two clipboard formats simultaneously:
//! 1. CF_HDROP — DROPFILES struct + UTF-16 file path array
//! 2. "Preferred DropEffect" — DWORD indicating copy (1) or move (2)

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

    // Register custom "Preferred DropEffect" format
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

    unsafe {
        // Open and empty clipboard
        OpenClipboard(None).map_err(|e| format!("OpenClipboard failed: {e}"))?;

        if let Err(e) = EmptyClipboard() {
            let _ = CloseClipboard();
            return Err(format!("EmptyClipboard failed: {e}"));
        }

        // Set CF_HDROP data
        let hdrop_hmem = alloc_global_data(&hdrop_data)?;
        let hdrop_handle = HANDLE(hdrop_hmem.0);
        if let Err(_e) = SetClipboardData(CF_HDROP, Some(hdrop_handle)) {
            let _ = CloseClipboard();
            return Err("SetClipboardData(CF_HDROP) failed".to_string());
        }

        // Set Preferred DropEffect data
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
    unsafe {
        // Check if CF_HDROP is available (returns Err if not available)
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
    // Get CF_HDROP data
    let handle = match unsafe { GetClipboardData(CF_HDROP) } {
        Ok(h) => h,
        Err(_) => return Ok(None),
    };
    let hdrop = HDROP(handle.0);

    // Query number of files
    let count = unsafe { DragQueryFileW(hdrop, 0xFFFFFFFF, None) };
    if count == 0 {
        return Ok(None);
    }

    let mut paths = Vec::with_capacity(count as usize);

    for i in 0..count {
        // Query required buffer length (returns char count excluding null)
        let len = unsafe { DragQueryFileW(hdrop, i, None) };
        if len == 0 {
            continue;
        }

        let mut buf = vec![0u16; (len + 1) as usize];
        unsafe { DragQueryFileW(hdrop, i, Some(&mut buf)) };

        // Convert UTF-16 to String (trim trailing null)
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        if !path.is_empty() {
            paths.push(path);
        }
    }

    if paths.is_empty() {
        return Ok(None);
    }

    // Check Preferred DropEffect to determine copy vs cut
    let op = unsafe { read_drop_effect() }.unwrap_or(FileClipboardOp::Copy);

    Ok(Some((paths, op)))
}

unsafe fn read_drop_effect() -> Option<FileClipboardOp> {
    let drop_effect_fmt = unsafe { RegisterClipboardFormatW(w!("Preferred DropEffect")) };
    if drop_effect_fmt == 0 {
        return None;
    }

    let handle = unsafe { GetClipboardData(drop_effect_fmt) }.ok()?;
    let hglobal = HGLOBAL(handle.0);
    let ptr = unsafe { GlobalLock(hglobal) };
    if ptr.is_null() {
        return None;
    }

    let size = unsafe { GlobalSize(hglobal) };
    let op = if size >= 4 {
        let value = unsafe { *(ptr as *const u32) };
        if value == 2 {
            FileClipboardOp::Cut // DROPEFFECT_MOVE
        } else {
            FileClipboardOp::Copy // DROPEFFECT_COPY (default)
        }
    } else {
        FileClipboardOp::Copy
    };

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
    unsafe {
        (*dropfiles).pFiles = dropfiles_size as u32;
        (*dropfiles).fWide = true.into();
        // pt and fNC are zero-initialized by default
    }

    // Copy UTF-16 path data after the header
    let path_offset = dropfiles_size;
    let wide_bytes =
        unsafe { std::slice::from_raw_parts(wide_data.as_ptr() as *const u8, wide_bytes_len) };
    data[path_offset..path_offset + wide_bytes_len].copy_from_slice(wide_bytes);

    Ok(data)
}

/// Allocate global memory and copy data into it.
/// Returns the HGLOBAL handle (ownership transferred to clipboard on SetClipboardData).
unsafe fn alloc_global_data(data: &[u8]) -> Result<HGLOBAL, String> {
    let hmem =
        unsafe { GlobalAlloc(GHND, data.len()) }.map_err(|e| format!("GlobalAlloc failed: {e}"))?;

    let ptr = unsafe { GlobalLock(hmem) };
    if ptr.is_null() {
        return Err("GlobalLock returned null".to_string());
    }

    unsafe { ptr::copy_nonoverlapping(data.as_ptr(), ptr as *mut u8, data.len()) };
    let _ = unsafe { GlobalUnlock(hmem) };

    Ok(hmem)
}
