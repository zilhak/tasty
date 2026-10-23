//! Read-only HTML entry handles. Path resolution and reads run only on transfer workers.
use std::{
    fs::File,
    path::{Component, Path, PathBuf},
    time::SystemTime,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Stamp {
    len: u64,
    #[cfg(windows)]
    identity: (u32, u32, u32),
    modified: Option<SystemTime>,
    created: Option<SystemTime>,
    #[cfg(unix)]
    dev: u64,
    #[cfg(unix)]
    ino: u64,
    #[cfg(unix)]
    ctime: (i64, i64),
}
impl Stamp {
    pub(crate) fn of(file: &File) -> std::io::Result<Self> {
        let m = file.metadata()?;
        #[cfg(windows)]
        let identity = {
            use std::os::windows::io::AsRawHandle;
            use windows::Win32::{
                Foundation::HANDLE,
                Storage::FileSystem::{BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle},
            };
            let mut info = BY_HANDLE_FILE_INFORMATION::default();
            // SAFETY: file owns the valid handle and info is writable for the call.
            unsafe { GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &mut info) }
                .map_err(std::io::Error::other)?;
            (
                info.dwVolumeSerialNumber,
                info.nFileIndexHigh,
                info.nFileIndexLow,
            )
        };
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;
        Ok(Self {
            len: m.len(),
            #[cfg(windows)]
            identity,
            modified: m.modified().ok(),
            created: m.created().ok(),
            #[cfg(unix)]
            dev: m.dev(),
            #[cfg(unix)]
            ino: m.ino(),
            #[cfg(unix)]
            ctime: (m.ctime(), m.ctime_nsec()),
        })
    }
}
pub(crate) struct Entry {
    pub(crate) file: File,
    pub(crate) root: PathBuf,
    pub(crate) stamp: Stamp,
    source: PathBuf,
}
impl Entry {
    pub(crate) fn open(source: &Path) -> Result<Self, String> {
        if !source.is_absolute() {
            return Err("entry source must be absolute".into());
        }
        let root = source
            .parent()
            .ok_or("entry has no parent")?
            .canonicalize()
            .map_err(|e| e.to_string())?;
        let target = source.canonicalize().map_err(|e| e.to_string())?;
        let relative = target
            .strip_prefix(&root)
            .map_err(|_| "entry symlink escapes document root")?;
        if !std::fs::metadata(&target)
            .map_err(|e| e.to_string())?
            .is_file()
        {
            return Err("HTML source is not a regular file".into());
        }
        let file = open_beneath(&root, relative).map_err(|e| e.to_string())?;
        let meta = file.metadata().map_err(|e| e.to_string())?;
        if !meta.is_file() {
            return Err("HTML source is not a regular file".into());
        }
        let stamp = Stamp::of(&file).map_err(|e| e.to_string())?;
        Ok(Self {
            file,
            root,
            stamp,
            source: source.to_owned(),
        })
    }
    /// Detect same-handle edits and path replacement before/after every credit.
    pub(crate) fn unchanged(&self) -> bool {
        Stamp::of(&self.file)
            .ok()
            .is_some_and(|stamp| stamp == self.stamp)
            && Self::open(&self.source)
                .ok()
                .is_some_and(|entry| entry.root == self.root && entry.stamp == self.stamp)
    }
}
#[cfg(unix)]
fn open_beneath(root: &Path, relative: &Path) -> std::io::Result<File> {
    use std::{
        ffi::CString,
        os::{
            fd::{AsRawFd, FromRawFd},
            unix::ffi::OsStrExt,
        },
    };
    let mut dir = File::open("/")?;
    let components: Vec<_> = root
        .components()
        .chain(relative.components())
        .filter(|c| !matches!(c, Component::RootDir))
        .collect();
    if components.is_empty() {
        return Err(std::io::Error::other("empty file path"));
    }
    for (i, c) in components.iter().enumerate() {
        let Component::Normal(name) = c else {
            return Err(std::io::Error::other("unsafe path component"));
        };
        let name = CString::new(name.as_bytes()).map_err(std::io::Error::other)?;
        let directory = i + 1 < components.len();
        let flags = libc::O_RDONLY
            | libc::O_CLOEXEC
            | libc::O_NOFOLLOW
            | libc::O_NONBLOCK
            | if directory { libc::O_DIRECTORY } else { 0 };
        // SAFETY: dir and NUL-terminated name remain alive. Each returned descriptor is owned once.
        let fd = unsafe { libc::openat(dir.as_raw_fd(), name.as_ptr(), flags) };
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: successful openat returned a fresh owned descriptor.
        dir = unsafe { File::from_raw_fd(fd) };
    }
    Ok(dir)
}
#[cfg(windows)]
fn open_beneath(root: &Path, relative: &Path) -> std::io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows::Win32::Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    };
    if !relative
        .components()
        .all(|c| matches!(c, Component::Normal(_)))
    {
        return Err(std::io::Error::other("unsafe path component"));
    }
    let root_handle = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS.0)
        .open(root)?;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0)
        .open(root.join(relative))?;
    let root_path = handle_path(&root_handle)?;
    let actual = handle_path(&file)?;
    if !actual.starts_with(root_path) {
        return Err(std::io::Error::other("opened file escapes root"));
    }
    Ok(file)
}
#[cfg(windows)]
fn handle_path(file: &File) -> std::io::Result<PathBuf> {
    use std::os::windows::{ffi::OsStringExt, io::AsRawHandle};
    use windows::Win32::{
        Foundation::HANDLE,
        Storage::FileSystem::{FILE_NAME_NORMALIZED, GetFinalPathNameByHandleW},
    };
    let mut buffer = vec![0u16; 32768];
    // SAFETY: the file handle and writable buffer remain valid for the call.
    let len = unsafe {
        GetFinalPathNameByHandleW(
            HANDLE(file.as_raw_handle()),
            &mut buffer,
            FILE_NAME_NORMALIZED,
        )
    } as usize;
    if len == 0 || len >= buffer.len() {
        return Err(std::io::Error::last_os_error());
    }
    Ok(PathBuf::from(std::ffi::OsString::from_wide(&buffer[..len])))
}
#[cfg(not(any(unix, windows)))]
fn open_beneath(_root: &Path, _relative: &Path) -> std::io::Result<File> {
    Err(std::io::Error::other("unsupported file handle backend"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn regular_handle_and_replacement() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("index.html");
        std::fs::write(&p, b"abc").unwrap();
        let e = Entry::open(&p).unwrap();
        assert!(e.unchanged());
        let other = d.path().join("new.html");
        std::fs::write(&other, b"def").unwrap();
        std::fs::rename(other, p).unwrap();
        assert!(!e.unchanged());
        assert!(Entry::open(d.path()).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn canonical_target_component_swap_cannot_open_an_outside_handle() {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("sub")).unwrap();
        std::fs::write(root.path().join("sub/a.html"), b"inside").unwrap();
        std::fs::write(outside.path().join("a.html"), b"outside").unwrap();
        let canonical = root.path().canonicalize().unwrap();
        let relative = Path::new("sub/a.html");
        // Simulate replacement after canonical root/target validation, immediately before openat.
        std::fs::rename(root.path().join("sub"), root.path().join("held")).unwrap();
        symlink(outside.path(), root.path().join("sub")).unwrap();
        assert!(open_beneath(&canonical, relative).is_err());
        std::fs::write(root.path().join("final.html"), b"inside").unwrap();
        std::fs::remove_file(root.path().join("final.html")).unwrap();
        symlink(
            outside.path().join("a.html"),
            root.path().join("final.html"),
        )
        .unwrap();
        assert!(open_beneath(&canonical, Path::new("final.html")).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn fifo_device_outside_link_and_inside_link() {
        use std::{
            ffi::CString,
            os::unix::{ffi::OsStrExt, fs::symlink},
        };
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("fifo");
        let name = CString::new(p.as_os_str().as_bytes()).unwrap();
        // SAFETY: name is NUL terminated and points inside the owned fixture.
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        assert!(Entry::open(&p).is_err());
        assert!(Entry::open(Path::new("/dev/null")).is_err());
        symlink("/etc/passwd", d.path().join("outside")).unwrap();
        assert!(Entry::open(&d.path().join("outside")).is_err());
        std::fs::write(d.path().join("ok"), b"ok").unwrap();
        symlink("ok", d.path().join("inside")).unwrap();
        assert!(Entry::open(&d.path().join("inside")).is_ok());
    }
}
