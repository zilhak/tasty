//! Windows file identity. Keep the source file open while relying on this identity:
//! closing the last handle after deletion permits the filesystem to reuse an ID.
use std::{fs::File, io, os::windows::io::AsRawHandle};

use windows_sys::Win32::Storage::FileSystem::{
    FILE_ID_INFO, FileIdInfo, GetFileInformationByHandleEx,
};

/// Volume serial plus the complete 128-bit file ID; no path or content fallback.
#[derive(Debug, PartialEq, Eq)]
pub struct FileIdentity {
    volume: u64,
    file_id: [u8; 16],
}

impl FileIdentity {
    /// Query a live file without changing its offset, access rights or share mode.
    /// Native errors and unusable zero file IDs are returned to the caller.
    pub fn of(file: &File) -> io::Result<Self> {
        let mut info = std::mem::MaybeUninit::<FILE_ID_INFO>::uninit();
        // SAFETY: the borrow keeps the owned handle live throughout this synchronous call;
        // info is aligned writable storage sized for exactly one FILE_ID_INFO.
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
        // SAFETY: success for FileIdInfo initialized the complete FILE_ID_INFO output.
        let info = unsafe { info.assume_init() };
        Self::from_parts(info.VolumeSerialNumber, info.FileId.Identifier)
    }

    fn from_parts(volume: u64, file_id: [u8; 16]) -> io::Result<Self> {
        if file_id == [0; 16] {
            return Err(io::Error::other(
                "filesystem did not supply a usable file ID",
            ));
        }
        Ok(Self { volume, file_id })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equality_uses_volume_and_all_128_file_id_bits_and_rejects_zero() {
        assert!(FileIdentity::from_parts(1, [0; 16]).is_err());
        let mut high_only = [0; 16];
        high_only[15] = 1;
        assert!(FileIdentity::from_parts(1, high_only).is_ok());
        let id = [1; 16];
        let original = FileIdentity::from_parts(1, id).unwrap();
        assert_eq!(original, FileIdentity::from_parts(1, id).unwrap());
        assert_ne!(original, FileIdentity::from_parts(2, id).unwrap());
        assert_ne!(
            original,
            FileIdentity::from_parts(1 + (1 << 32), id).unwrap()
        );
        for byte in 0..16 {
            let mut changed = id;
            changed[byte] ^= 0x80;
            assert_ne!(original, FileIdentity::from_parts(1, changed).unwrap());
        }
    }

    #[test]
    fn retained_handle_allows_replace_unlink_and_distinguishes_identical_contents() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("publication");
        std::fs::write(&path, b"4242").unwrap();
        let old = File::open(&path).unwrap();
        let original = FileIdentity::of(&old).unwrap();
        assert_eq!(
            original,
            FileIdentity::of(&File::open(&path).unwrap()).unwrap()
        );
        let next = dir.path().join("replacement");
        std::fs::write(&next, b"4242").unwrap();
        std::fs::rename(&next, &path).unwrap();
        let current = File::open(&path).unwrap();
        assert_ne!(original, FileIdentity::of(&current).unwrap());
        assert_eq!(original, FileIdentity::of(&old).unwrap());
        std::fs::remove_file(&path).unwrap();
        assert_eq!(original, FileIdentity::of(&old).unwrap());
    }

    #[test]
    fn native_query_failure_is_returned_for_a_nonfilesystem_handle() {
        let device = File::open("NUL").unwrap();
        assert!(FileIdentity::of(&device).is_err());
    }
}
