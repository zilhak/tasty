//! Preserve access controls before putting content into a replacement file.
use std::{
    fs::{File, Metadata},
    io,
};

#[cfg(unix)]
pub(super) fn preserve(source: &File, target: &File, metadata: &Metadata) -> io::Result<()> {
    use std::os::unix::fs::MetadataExt;
    let next = target.metadata()?;
    if (next.uid(), next.gid()) != (metadata.uid(), metadata.gid()) {
        rustix::fs::fchown(
            target,
            Some(rustix::fs::Uid::from_raw(metadata.uid())),
            Some(rustix::fs::Gid::from_raw(metadata.gid())),
        )?;
    }
    target.set_permissions(metadata.permissions())?;
    copy_attributes(source, target)
}

#[cfg(target_os = "linux")]
fn attributes(file: &File) -> io::Result<Vec<(std::ffi::CString, Vec<u8>)>> {
    let len = match rustix::fs::flistxattr(file, &mut [] as &mut [u8]) {
        Ok(len) => len,
        Err(rustix::io::Errno::OPNOTSUPP) => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    let mut names = vec![0u8; len];
    let len = rustix::fs::flistxattr(file, names.as_mut_slice())?;
    names.truncate(len);
    let mut attrs = Vec::new();
    for name in names.split(|b| *b == 0).filter(|s| !s.is_empty()) {
        let name = std::ffi::CString::new(name).map_err(io::Error::other)?;
        let len = rustix::fs::fgetxattr(file, &name, &mut [] as &mut [u8])?;
        let mut value = vec![0u8; len];
        let len = rustix::fs::fgetxattr(file, &name, value.as_mut_slice())?;
        value.truncate(len);
        attrs.push((name, value));
    }
    Ok(attrs)
}

#[cfg(target_os = "linux")]
fn copy_attributes(source: &File, target: &File) -> io::Result<()> {
    let source = attributes(source)?;
    let current = attributes(target)?;
    for (name, _) in &current {
        if !source.iter().any(|(other, _)| other == name) {
            rustix::fs::fremovexattr(target, name)?;
        }
    }
    for (name, value) in source {
        if current.iter().any(|(n, v)| n == &name && v == &value) {
            continue;
        }
        rustix::fs::fsetxattr(target, &name, &value, rustix::fs::XattrFlags::empty())?;
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn copy_attributes(source: &File, target: &File) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    // SAFETY: both owned files remain open; null requests copyfile's temporary state.
    if unsafe {
        libc::fcopyfile(
            source.as_raw_fd(),
            target.as_raw_fd(),
            std::ptr::null_mut(),
            libc::COPYFILE_ACL | libc::COPYFILE_XATTR,
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
fn copy_attributes(_source: &File, _target: &File) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "atomic replacement metadata is unsupported on this platform",
    ))
}

#[cfg(not(unix))]
pub(super) fn preserve(_source: &File, target: &File, metadata: &Metadata) -> io::Result<()> {
    target.set_permissions(metadata.permissions())
}

#[cfg(windows)]
pub(super) fn create(
    path: &std::path::Path,
    source: &std::path::Path,
    existing: bool,
) -> io::Result<File> {
    use std::os::windows::{ffi::OsStrExt, io::FromRawHandle};
    use windows_sys::Win32::{
        Foundation::{GENERIC_WRITE, INVALID_HANDLE_VALUE},
        Security::{
            DACL_SECURITY_INFORMATION, GROUP_SECURITY_INFORMATION, GetFileSecurityW,
            OWNER_SECURITY_INFORMATION, SECURITY_ATTRIBUTES,
        },
        Storage::FileSystem::{CREATE_NEW, CreateFileW, FILE_ATTRIBUTE_NORMAL},
    };
    let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let flags = OWNER_SECURITY_INFORMATION | GROUP_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION;
    let mut length = 0;
    let mut descriptor = Vec::new();
    if existing {
        // SAFETY: source is NUL terminated; the null output asks for the required capacity.
        unsafe {
            GetFileSecurityW(source.as_ptr(), flags, std::ptr::null_mut(), 0, &mut length);
        }
        if length == 0 {
            return Err(io::Error::last_os_error());
        }
        descriptor.resize(length as usize, 0u8);
        // SAFETY: descriptor is writable for length bytes and lives through CreateFileW.
        if unsafe {
            GetFileSecurityW(
                source.as_ptr(),
                flags,
                descriptor.as_mut_ptr().cast(),
                length,
                &mut length,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
    }
    let security = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: if existing {
            descriptor.as_mut_ptr().cast()
        } else {
            std::ptr::null_mut()
        },
        bInheritHandle: 0,
    };
    // SAFETY: all pointers refer to live allocations; CREATE_NEW cannot replace an existing file.
    let handle = unsafe {
        CreateFileW(
            path.as_ptr(),
            GENERIC_WRITE,
            0,
            &security,
            CREATE_NEW,
            FILE_ATTRIBUTE_NORMAL,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: CreateFileW returned a uniquely owned valid handle.
    Ok(unsafe { File::from_raw_handle(handle) })
}
