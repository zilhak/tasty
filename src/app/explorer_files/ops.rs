//! Name helpers and OS rename primitives shared by the job runner.
//! Publication never replaces an entry unless the job runner was told to replace a file.

use std::io;
use std::path::{Path, PathBuf};

/// Choose a display name; the actual publication still atomically rejects a competing entry.
pub fn unique_dest(dest_dir: &Path, name: &str) -> PathBuf {
    let candidate = dest_dir.join(name);
    if matches!(candidate.symlink_metadata(), Err(e) if e.kind() == io::ErrorKind::NotFound) {
        return candidate;
    }
    let p = Path::new(name);
    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or(name);
    let ext = p.extension().and_then(|s| s.to_str());
    for i in 1u64.. {
        let suffix = if i == 1 {
            " (copy)".to_string()
        } else {
            format!(" (copy {i})")
        };
        let fname = match ext {
            Some(e) => format!("{stem}{suffix}.{e}"),
            None => format!("{stem}{suffix}"),
        };
        let candidate = dest_dir.join(fname);
        match candidate.symlink_metadata() {
            Err(e) if e.kind() == io::ErrorKind::NotFound => return candidate,
            // Let publication report access errors instead of searching names forever.
            Err(_) => return candidate,
            Ok(_) => {}
        }
    }
    unreachable!("file name space exhausted")
}

/// A rename is one filename in the original directory, never an overwrite or path move.
pub fn rename_entry(src: &Path, name: &str) -> io::Result<()> {
    let mut components = Path::new(name).components();
    if !matches!(components.next(), Some(std::path::Component::Normal(_)))
        || components.next().is_some()
        || name.contains(['/', '\\'])
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "expected one file name",
        ));
    }
    let parent = src
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no parent"))?;
    let dst = parent.join(name);
    if dst == src {
        return Ok(());
    }
    rename_noreplace(src, &dst)
}

/// A new entry is one filename in `dir`. Creation fails instead of replacing an existing entry.
pub fn create_entry(dir: &Path, name: &str, folder: bool) -> io::Result<()> {
    let mut components = Path::new(name).components();
    if !matches!(components.next(), Some(std::path::Component::Normal(_)))
        || components.next().is_some()
        || name.contains(['/', '\\'])
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "expected one file name",
        ));
    }
    let path = dir.join(name);
    if folder {
        std::fs::create_dir(&path)
    } else {
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map(drop)
    }
}

pub fn remove_path(path: &Path) -> io::Result<()> {
    if path.symlink_metadata()?.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
}

/// OS primitives preserve an entry created concurrently, including dangling symlinks.
pub(super) fn rename_noreplace(src: &Path, dst: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let src = std::ffi::CString::new(src.as_os_str().as_bytes())?;
        let dst = std::ffi::CString::new(dst.as_os_str().as_bytes())?;
        // SAFETY: both C strings are valid for the call; the OS owns no borrowed pointers.
        #[cfg(target_os = "linux")]
        let result = unsafe {
            libc::renameat2(
                libc::AT_FDCWD,
                src.as_ptr(),
                libc::AT_FDCWD,
                dst.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        };
        // SAFETY: same lifetime contract as the Linux call, with macOS exclusive rename semantics.
        #[cfg(target_os = "macos")]
        let result = unsafe { libc::renamex_np(src.as_ptr(), dst.as_ptr(), libc::RENAME_EXCL) };
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "exclusive rename is unavailable",
        ));
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        if result == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows::Win32::Storage::FileSystem::{MOVE_FILE_FLAGS, MoveFileExW};
        let src: Vec<u16> = src.as_os_str().encode_wide().chain(Some(0)).collect();
        let dst: Vec<u16> = dst.as_os_str().encode_wide().chain(Some(0)).collect();
        if src[..src.len() - 1].contains(&0) || dst[..dst.len() - 1].contains(&0) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "path contains NUL",
            ));
        }
        // SAFETY: nul-terminated buffers live through the call; zero flags forbid replacement.
        unsafe {
            MoveFileExW(
                windows::core::PCWSTR(src.as_ptr()),
                windows::core::PCWSTR(dst.as_ptr()),
                MOVE_FILE_FLAGS(0),
            )
        }
        .map_err(|error| io::Error::from_raw_os_error(error.code().0 & 0xffff))
    }
}

#[cfg(test)]
// 테스트는 의도적으로 무시하는 결과가 많아 let _ 사유 검사에서 제외한다.
#[allow(clippy::let_underscore_must_use)]
mod tests {
    use super::*;

    /// 묻지 않고 Keep both 로 처리하는 작업 하나. 만든 경로나 첫 실패를 돌려준다.
    fn transfer(src: &Path, dest_dir: &Path, cut: bool) -> io::Result<PathBuf> {
        use super::super::job::{Choice, Shared, UndoStep, run_transfer};
        let report = run_transfer(
            &Shared::fixed(Choice::KeepBoth),
            &[src.to_path_buf()],
            dest_dir,
            cut,
        );
        if let Some(failure) = report.failed.first() {
            return Err(io::Error::other(format!("{:?}", failure.reason)));
        }
        Ok(match report.undo.first() {
            Some(UndoStep::Created(p, _) | UndoStep::Replaced(p)) => p.clone(),
            Some(UndoStep::Moved { to, .. }) => to.clone(),
            None => src.to_path_buf(),
        })
    }

    #[test]
    fn unique_dest_appends_copy_suffix() {
        let dir = std::env::temp_dir().join(format!("tasty_ops_uniq_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let a = dir.join("a.txt");
        std::fs::write(&a, b"x").unwrap();
        let d1 = unique_dest(&dir, "a.txt");
        assert_eq!(d1, dir.join("a (copy).txt"));
        std::fs::write(&d1, b"x").unwrap();
        let d2 = unique_dest(&dir, "a.txt");
        assert_eq!(d2, dir.join("a (copy 2).txt"));
        assert_eq!(unique_dest(&dir, "z.txt"), dir.join("z.txt"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn transfer_rejects_paste_into_self() {
        let dir = std::env::temp_dir().join(format!("tasty_ops_self_{}", std::process::id()));
        let sub = dir.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        assert!(transfer(&dir, &sub, true).is_err());
        assert!(transfer(&dir, &dir, false).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn copy_then_delete() {
        let dir = std::env::temp_dir().join(format!("tasty_ops_copy_{}", std::process::id()));
        let dst = dir.join("dst");
        std::fs::create_dir_all(&dst).unwrap();
        let f = dir.join("f.txt");
        std::fs::write(&f, b"hello").unwrap();
        let out = transfer(&f, &dst, false).unwrap();
        assert!(out.exists());
        assert!(f.exists()); // 복사라 원본 유지.
        remove_path(&out).unwrap();
        assert!(!out.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
    #[test]
    fn rename_collision_and_path_escape_preserve_both_files() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("a");
        let dst = dir.path().join("b");
        std::fs::write(&src, b"a").unwrap();
        std::fs::write(&dst, b"b").unwrap();
        assert_eq!(
            rename_entry(&src, "b").unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        #[cfg(windows)]
        for name in ["C:escape", "C:"] {
            assert!(rename_entry(&src, name).is_err());
        }
        for name in ["../b", "sub/b", "..", ".", ""] {
            assert!(rename_entry(&src, name).is_err());
        }
        assert_eq!(std::fs::read(&src).unwrap(), b"a");
        assert_eq!(std::fs::read(&dst).unwrap(), b"b");
    }

    #[cfg(unix)]
    #[test]
    fn copy_preserves_dangling_destination_and_source_links() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("dest");
        std::fs::create_dir(&dest).unwrap();
        let src = dir.path().join("value");
        std::fs::write(&src, b"data").unwrap();
        let outside = dir.path().join("outside");
        symlink(&outside, dest.join("value")).unwrap();
        let copied = transfer(&src, &dest, false).unwrap();
        // transfer publishes under the canonical destination; macOS temp dirs sit behind /var -> /private/var.
        assert_eq!(copied, dest.canonicalize().unwrap().join("value (copy)"));
        assert!(!outside.exists());
        assert!(
            dest.join("value")
                .symlink_metadata()
                .unwrap()
                .file_type()
                .is_symlink()
        );
        let link = dir.path().join("link");
        symlink("missing", &link).unwrap();
        let copied = transfer(&link, &dest, false).unwrap();
        assert_eq!(
            std::fs::read_link(copied).unwrap(),
            PathBuf::from("missing")
        );
    }

    #[cfg(unix)]
    #[test]
    fn directory_alias_is_rejected_before_creating_any_copy() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("source");
        let inner = src.join("inner");
        std::fs::create_dir_all(&inner).unwrap();
        let alias = dir.path().join("alias");
        std::os::unix::fs::symlink(&inner, &alias).unwrap();
        for cut in [false, true] {
            assert!(transfer(&src, &alias, cut).is_err());
        }
        assert_eq!(std::fs::read_dir(inner).unwrap().count(), 0);
    }

    #[cfg(unix)]
    #[test]
    fn recursive_copy_preserves_cycle_as_link_and_cleans_failed_staging() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("source");
        let dest = dir.path().join("dest");
        std::fs::create_dir(&src).unwrap();
        std::fs::create_dir(&dest).unwrap();
        std::os::unix::fs::symlink(".", src.join("cycle")).unwrap();
        let copy = transfer(&src, &dest, false).unwrap();
        assert_eq!(
            std::fs::read_link(copy.join("cycle")).unwrap(),
            PathBuf::from(".")
        );
        let socket = src.join("socket");
        let _listener = std::os::unix::net::UnixListener::bind(socket).unwrap();
        assert!(transfer(&src, &dest, false).is_err());
        assert_eq!(std::fs::read_dir(&dest).unwrap().count(), 1);
        assert!(src.exists());
    }

    #[cfg(unix)]
    #[test]
    fn renaming_or_removing_a_folder_link_leaves_the_target_folder() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real");
        std::fs::create_dir(&real).unwrap();
        std::fs::write(real.join("keep.txt"), b"k").unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        rename_entry(&link, "renamed").unwrap();
        let renamed = dir.path().join("renamed");
        assert!(renamed.symlink_metadata().unwrap().file_type().is_symlink());
        remove_path(&renamed).unwrap();
        assert!(renamed.symlink_metadata().is_err());
        assert_eq!(std::fs::read(real.join("keep.txt")).unwrap(), b"k");
    }
}
