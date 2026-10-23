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
    Windows(u64, [u8; 16]),
}

#[cfg(unix)]
fn identity(file: &File) -> io::Result<FileIdentity> {
    use std::os::unix::fs::MetadataExt;
    let meta = file.metadata()?;
    Ok(FileIdentity::Unix(meta.dev(), meta.ino()))
}

#[cfg(windows)]
#[allow(
    unsafe_code,
    reason = "the Windows stable std metadata API lacks file IDs; this bounded call compares retained open handles using the full 128-bit identity"
)]
fn identity(file: &File) -> io::Result<FileIdentity> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ID_INFO, FileIdInfo, GetFileInformationByHandleEx,
    };
    let mut info = std::mem::MaybeUninit::<FILE_ID_INFO>::uninit();
    // SAFETY: file owns the live handle; info points to one correctly sized output structure.
    let ok = unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileIdInfo,
            info.as_mut_ptr().cast(),
            std::mem::size_of::<FILE_ID_INFO>() as u32,
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the successful API call initialized the complete FILE_ID_INFO output.
    let info = unsafe { info.assume_init() };
    if info.FileId.Identifier == [0; 16] {
        return Err(io::Error::other(
            "filesystem did not supply a usable file ID",
        ));
    }
    Ok(FileIdentity::Windows(
        info.VolumeSerialNumber,
        info.FileId.Identifier,
    ))
}

#[cfg(not(any(unix, windows)))]
fn identity(_file: &File) -> io::Result<FileIdentity> {
    Err(io::ErrorKind::Unsupported.into())
}
