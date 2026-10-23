//! Read-only HTML entry handles. Path resolution and reads run only on transfer workers.
#[cfg(unix)]
mod unix_root;
#[cfg(windows)]
mod windows_root;

use std::sync::Arc;
#[cfg(unix)]
use unix_root::Root;
#[cfg(windows)]
use windows_root::Root;

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
    authorized_root: Arc<Root>,
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
        let authorized_root = Arc::new(Root::open(&root).map_err(|e| e.to_string())?);
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
        let file = authorized_root
            .open_file(relative)
            .map_err(|e| e.to_string())?;
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
            authorized_root,
        })
    }
    /// Resolve a resource through the same originally authorized directory object as this entry.
    /// Call only on a transfer worker; no browser/client path is trusted as an absolute source.
    pub(crate) fn resource(&self, relative: &str) -> Result<Self, String> {
        let path = Path::new(relative);
        if relative.is_empty()
            || relative.contains(['\\', '\0'])
            || relative
                .split('/')
                .any(|part| part.is_empty() || matches!(part, "." | ".."))
            || !path.components().all(|c| matches!(c, Component::Normal(_)))
        {
            return Err("unsafe HTML resource path".into());
        }
        if !self.unchanged() {
            return Err("HTML entry/root changed; request a fresh document".into());
        }
        let source = self.root.join(path);
        let target = source.canonicalize().map_err(|e| e.to_string())?;
        let resolved = target
            .strip_prefix(&self.root)
            .map_err(|_| "HTML resource symlink escapes document root")?;
        if !std::fs::metadata(&target)
            .map_err(|e| e.to_string())?
            .is_file()
        {
            return Err("HTML resource is not a regular file".into());
        }
        let file = self
            .authorized_root
            .open_file(resolved)
            .map_err(|e| e.to_string())?;
        if !file.metadata().map_err(|e| e.to_string())?.is_file() {
            return Err("HTML resource is not a regular file".into());
        }
        let stamp = Stamp::of(&file).map_err(|e| e.to_string())?;
        let resource = Self {
            file,
            root: self.root.clone(),
            stamp,
            source,
            authorized_root: Arc::clone(&self.authorized_root),
        };
        if !self.unchanged() || !resource.unchanged() {
            return Err("HTML resource changed while opening".into());
        }
        Ok(resource)
    }
    /// Detect same-handle edits and path replacement before/after every credit.
    pub(crate) fn unchanged(&self) -> bool {
        Stamp::of(&self.file)
            .ok()
            .is_some_and(|stamp| stamp == self.stamp)
            && self.source_unchanged()
    }
    fn source_unchanged(&self) -> bool {
        let check = || -> std::io::Result<bool> {
            if !self.authorized_root.is_current(&self.root)? {
                return Ok(false);
            }
            let target = self.source.canonicalize()?;
            let relative = target
                .strip_prefix(&self.root)
                .map_err(std::io::Error::other)?;
            let file = self.authorized_root.open_file(relative)?;
            Ok(Stamp::of(&file)? == self.stamp)
        };
        check().unwrap_or(false)
    }
}
#[cfg(all(unix, test))]
fn open_beneath(root: &Path, relative: &Path) -> std::io::Result<File> {
    Root::open(root)?.open_file(relative)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resource_uses_original_root_and_refuses_unsafe_or_changed_sources() {
        use std::io::Read;
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("index.html"), b"entry").unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("sub/theme.css"), b"body{color:red}").unwrap();
        let entry = Entry::open(&dir.path().join("index.html")).unwrap();
        let mut resource = entry.resource("sub/theme.css").unwrap();
        let mut bytes = String::new();
        resource.file.read_to_string(&mut bytes).unwrap();
        assert_eq!(bytes, "body{color:red}");
        for invalid in [
            "",
            "../index.html",
            "/etc/passwd",
            "sub/../index.html",
            "sub//theme.css",
            "./index.html",
            "sub\\theme.css",
            "sub/",
            "sub\0theme.css",
        ] {
            assert!(entry.resource(invalid).is_err(), "{invalid:?}");
        }
        assert!(entry.resource("sub").is_err());
        std::fs::write(dir.path().join("replacement"), b"changed").unwrap();
        std::fs::rename(
            dir.path().join("replacement"),
            dir.path().join("index.html"),
        )
        .unwrap();
        assert!(entry.resource("sub/theme.css").is_err());
    }
    #[cfg(unix)]
    #[test]
    fn resource_links_and_root_replacement_never_read_outside() {
        use std::{io::Read, os::unix::fs::symlink};
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let root = dir.path().join("root");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("index.html"), b"entry").unwrap();
        std::fs::write(root.join("inside.css"), b"inside").unwrap();
        std::fs::write(outside.path().join("inside.css"), vec![b'X'; 48 * 1024]).unwrap();
        symlink("inside.css", root.join("alias.css")).unwrap();
        symlink(outside.path(), root.join("escape")).unwrap();
        let entry = Entry::open(&root.join("index.html")).unwrap();
        let mut resource = entry.resource("alias.css").unwrap();
        let mut bytes = String::new();
        resource.file.read_to_string(&mut bytes).unwrap();
        assert_eq!(bytes, "inside");
        assert!(entry.resource("escape/inside.css").is_err());
        std::fs::rename(&root, dir.path().join("held")).unwrap();
        symlink(outside.path(), &root).unwrap();
        assert!(!entry.unchanged());
        assert!(!resource.unchanged());
        assert!(entry.resource("inside.css").is_err());
        // Even after pathname replacement, the original root handle can only read inside bytes.
        let mut pinned = entry
            .authorized_root
            .open_file(Path::new("inside.css"))
            .unwrap();
        let mut original = String::new();
        pinned.read_to_string(&mut original).unwrap();
        assert_eq!(original, "inside");
    }
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
