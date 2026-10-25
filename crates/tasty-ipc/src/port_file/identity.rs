//! A retained open file prevents a removed publication's filesystem ID from being reused.
use std::{fs::File, io, path::Path};

#[derive(Debug)]
pub(super) struct PinnedFile {
    _file: File,
    id: FileIdentity,
}

impl PinnedFile {
    pub(super) fn open(path: &Path, contents: &str) -> io::Result<Self> {
        // std's Windows default share mode includes DELETE, so this pin permits replace/unlink.
        if !std::fs::metadata(path)?.is_file() {
            return Err(io::Error::other("port publication is not a regular file"));
        }
        let file = File::open(path)?;
        if !file.metadata()?.is_file() || !super::has_contents(&file, contents)? {
            return Err(io::Error::other(
                "port publication changed before identity capture",
            ));
        }
        let id = identity(&file)?;
        Ok(Self { _file: file, id })
    }

    pub(super) fn matches(&self, current: &File) -> io::Result<bool> {
        Ok(self.id == identity(current)?)
    }
}

#[derive(Debug, PartialEq, Eq)]
enum FileIdentity {
    #[cfg(unix)]
    Unix(u64, u64),
    #[cfg(windows)]
    Windows(tasty_utils::windows_file_identity::FileIdentity),
}

#[cfg(unix)]
fn identity(file: &File) -> io::Result<FileIdentity> {
    use std::os::unix::fs::MetadataExt;
    let meta = file.metadata()?;
    Ok(FileIdentity::Unix(meta.dev(), meta.ino()))
}

#[cfg(windows)]
fn identity(file: &File) -> io::Result<FileIdentity> {
    tasty_utils::windows_file_identity::FileIdentity::of(file).map(FileIdentity::Windows)
}

#[cfg(not(any(unix, windows)))]
fn identity(_file: &File) -> io::Result<FileIdentity> {
    Err(io::ErrorKind::Unsupported.into())
}
