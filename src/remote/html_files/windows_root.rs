//! Keep the authorized directory object across reads; never re-authorize a replacement root.
use super::{Component, File, Path, Stamp};
use std::{
    io,
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle},
    },
};
use windows::{
    Wdk::{
        Foundation::OBJECT_ATTRIBUTES,
        Storage::FileSystem::{
            FILE_DIRECTORY_FILE, FILE_NON_DIRECTORY_FILE, FILE_OPEN, FILE_SYNCHRONOUS_IO_NONALERT,
            NtCreateFile,
        },
    },
    Win32::{
        Foundation::{HANDLE, OBJ_CASE_INSENSITIVE, OBJ_DONT_REPARSE, UNICODE_STRING},
        Storage::FileSystem::{
            FILE_ATTRIBUTE_NORMAL, FILE_GENERIC_READ, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE,
            FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_TRAVERSE, SYNCHRONIZE,
        },
        System::IO::IO_STATUS_BLOCK,
    },
};

pub(super) struct Root {
    handle: File,
    identity: (u32, u32, u32),
}
impl Root {
    pub(super) fn open(path: &Path) -> io::Result<Self> {
        // canonicalize returns a verbatim DOS/UNC path. Convert only its namespace prefix;
        // NtCreateFile, unlike a final-component OPEN_REPARSE_POINT, rejects *all* reparses.
        let mut name: Vec<u16> = path.as_os_str().encode_wide().collect();
        if !name.starts_with(&[92, 92, 63, 92]) {
            return Err(io::Error::other("root is not a canonical verbatim path"));
        }
        name[1] = 63;
        let handle = open(None, &mut name, true)?;
        let identity = Stamp::of(&handle)?.identity;
        Ok(Self { handle, identity })
    }
    pub(super) fn is_current(&self, path: &Path) -> io::Result<bool> {
        Ok(Self::open(path)?.identity == self.identity)
    }
    pub(super) fn open_file(&self, relative: &Path) -> io::Result<File> {
        if relative.as_os_str().is_empty()
            || !relative
                .components()
                .all(|c| matches!(c, Component::Normal(_)))
        {
            return Err(io::Error::other("unsafe path component"));
        }
        let mut name: Vec<u16> = relative.as_os_str().encode_wide().collect();
        // RootDirectory is the original handle, even if its pathname/ancestor was renamed.
        // OBJ_DONT_REPARSE also rejects an intermediate junction inside this relative name.
        open(Some(&self.handle), &mut name, false)
    }
}
fn open(root: Option<&File>, name: &mut [u16], directory: bool) -> io::Result<File> {
    let bytes = name
        .len()
        .checked_mul(2)
        .and_then(|n| u16::try_from(n).ok())
        .ok_or_else(|| io::Error::other("file name too long"))?;
    if name.contains(&0) {
        return Err(io::Error::other("NUL in file name"));
    }
    let unicode = UNICODE_STRING {
        Length: bytes,
        MaximumLength: bytes,
        Buffer: windows::core::PWSTR(name.as_mut_ptr()),
    };
    let attributes = OBJECT_ATTRIBUTES {
        Length: std::mem::size_of::<OBJECT_ATTRIBUTES>() as u32,
        RootDirectory: root.map_or(HANDLE::default(), |f| HANDLE(f.as_raw_handle())),
        ObjectName: &unicode,
        Attributes: OBJ_CASE_INSENSITIVE | OBJ_DONT_REPARSE,
        ..Default::default()
    };
    let mut handle = HANDLE::default();
    let mut status = IO_STATUS_BLOCK::default();
    let access = if directory {
        FILE_READ_ATTRIBUTES | FILE_TRAVERSE | SYNCHRONIZE
    } else {
        FILE_GENERIC_READ
    };
    let kind = if directory {
        FILE_DIRECTORY_FILE
    } else {
        FILE_NON_DIRECTORY_FILE
    };
    // SAFETY: attributes/unicode/name and both output slots remain valid for this synchronous
    // call. FILE_OPEN never creates anything; success returns exactly one owned handle.
    let result = unsafe {
        NtCreateFile(
            &mut handle,
            access,
            &attributes,
            &mut status,
            None,
            FILE_ATTRIBUTE_NORMAL,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            FILE_OPEN,
            kind | FILE_SYNCHRONOUS_IO_NONALERT,
            None,
            0,
        )
    };
    result.ok().map_err(io::Error::other)?;
    // SAFETY: NtCreateFile succeeded and transferred ownership of its handle to this File.
    Ok(unsafe { File::from_raw_handle(handle.0) })
}

#[cfg(test)]
#[path = "windows_root_tests.rs"]
mod tests;
