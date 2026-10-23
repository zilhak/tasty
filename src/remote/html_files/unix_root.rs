//! Directory-relative opens retain the authorized root object and reject every symlink hop.
use super::{Component, File, Path};
use std::{
    ffi::CString,
    io,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{ffi::OsStrExt, fs::MetadataExt},
    },
};

pub(super) struct Root {
    handle: File,
    identity: (u64, u64),
}
impl Root {
    pub(super) fn open(path: &Path) -> io::Result<Self> {
        if !path.is_absolute() {
            return Err(io::Error::other("root must be absolute"));
        }
        let root = File::open("/")?;
        let relative = path.strip_prefix("/").map_err(io::Error::other)?;
        let handle = if relative.as_os_str().is_empty() {
            root
        } else {
            walk(&root, relative, true)?
        };
        let metadata = handle.metadata()?;
        Ok(Self {
            handle,
            identity: (metadata.dev(), metadata.ino()),
        })
    }
    pub(super) fn is_current(&self, path: &Path) -> io::Result<bool> {
        Ok(Self::open(path)?.identity == self.identity)
    }
    pub(super) fn open_file(&self, relative: &Path) -> io::Result<File> {
        walk(&self.handle, relative, false)
    }
}
fn walk(root: &File, relative: &Path, directory: bool) -> io::Result<File> {
    let components: Vec<_> = relative.components().collect();
    if components.is_empty() {
        return Err(io::Error::other("empty file path"));
    }
    let mut current = root.try_clone()?;
    for (index, component) in components.iter().enumerate() {
        let Component::Normal(name) = component else {
            return Err(io::Error::other("unsafe path component"));
        };
        let name = CString::new(name.as_bytes()).map_err(io::Error::other)?;
        let is_directory = directory || index + 1 < components.len();
        let flags = libc::O_RDONLY
            | libc::O_CLOEXEC
            | libc::O_NOFOLLOW
            | libc::O_NONBLOCK
            | if is_directory { libc::O_DIRECTORY } else { 0 };
        // SAFETY: current owns the directory; the NUL-terminated name outlives the call.
        let fd = unsafe { libc::openat(current.as_raw_fd(), name.as_ptr(), flags) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: successful openat returns a fresh descriptor transferred to this File once.
        current = unsafe { File::from_raw_fd(fd) };
    }
    Ok(current)
}
