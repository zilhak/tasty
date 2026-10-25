//! Prepare only the app-owned namespace leaf; never follow links or chmod shared ancestors.
use super::super::profile::ProfileError;
use std::{
    ffi::{CString, OsStr},
    fs::File,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{ffi::OsStrExt, fs::MetadataExt},
    },
    path::{Component, Path, PathBuf},
};
fn uid() -> u32 {
    // SAFETY: geteuid has no arguments or memory preconditions.
    unsafe { libc::geteuid() }
}
fn open(parent: &File, name: &OsStr) -> Result<File, ProfileError> {
    let name = CString::new(name.as_bytes()).map_err(|_| ProfileError::Invalid)?;
    // SAFETY: live directory descriptor and single NUL-terminated relative path component.
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    // SAFETY: successful openat returned a fresh descriptor owned once by File.
    Ok(unsafe { File::from_raw_fd(fd) })
}
fn ancestor(dir: &File) -> Result<(), ProfileError> {
    let m = dir.metadata()?;
    if !m.is_dir()
        || (m.uid() != uid() && m.uid() != 0)
        || (m.mode() & 0o022 != 0 && !(m.uid() == 0 && m.mode() & 0o1000 != 0))
    {
        return Err(ProfileError::Ownership);
    }
    Ok(())
}
fn home(path: &Path) -> Result<File, ProfileError> {
    if !path.is_absolute() {
        return Err(ProfileError::Invalid);
    }
    let mut dir = File::open("/")?;
    for part in path.components() {
        match part {
            Component::RootDir => {}
            Component::Normal(name) => {
                dir = open(&dir, name)?;
                ancestor(&dir)?;
            }
            _ => return Err(ProfileError::Invalid),
        }
    }
    if dir.metadata()?.uid() != uid() {
        return Err(ProfileError::Ownership);
    }
    Ok(dir)
}
pub(super) fn prepare(data_home: &Path) -> Result<PathBuf, ProfileError> {
    let parent = home(data_home)?;
    let name = OsStr::new("native-http-profiles");
    let c = CString::new(name.as_bytes()).map_err(|_| ProfileError::Invalid)?;
    // SAFETY: parent is an owned directory fd, c is a fixed single relative name.
    let created = unsafe { libc::mkdirat(parent.as_raw_fd(), c.as_ptr(), 0o700) } == 0;
    if !created && std::io::Error::last_os_error().kind() != std::io::ErrorKind::AlreadyExists {
        return Err(std::io::Error::last_os_error().into());
    }
    if created {
        // SAFETY: the parent fd is pinned and the fixed leaf cannot redirect through a symlink.
        // Set explicit permissions before opening: a restrictive umask can remove read access.
        if unsafe {
            libc::fchmodat(
                parent.as_raw_fd(),
                c.as_ptr(),
                0o700,
                libc::AT_SYMLINK_NOFOLLOW,
            )
        } < 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
    }
    let dir = open(&parent, name)?;
    let before = dir.metadata()?;
    if before.uid() != uid() {
        return Err(ProfileError::Ownership);
    }
    let m = dir.metadata()?;
    if m.mode() & 0o7777 != 0o700 {
        return Err(ProfileError::Ownership);
    }
    // Reopen every ancestor and leaf and compare actual objects before returning a pathname.
    let current = home(data_home)?;
    let a = parent.metadata()?;
    let b = current.metadata()?;
    let current = open(&current, name)?.metadata()?;
    if (a.dev(), a.ino(), a.uid()) != (b.dev(), b.ino(), b.uid())
        || (m.dev(), m.ino(), m.uid()) != (current.dev(), current.ino(), current.uid())
    {
        return Err(ProfileError::Ownership);
    }
    Ok(data_home.join(name))
}
