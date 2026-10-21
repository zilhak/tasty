//! Properties 권한 편집이 쓰는 권한 변경. 고른 항목 하나에만 적용하고 폴더 안으로 내려가지 않는다.
//! 권한을 올려 달라고 OS에 요청하지 않으므로 거절되면 OS 오류를 그대로 돌려준다. 링크는 바꾸지 않는다.

use std::path::Path;

/// 바꿀 권한.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PermChange {
    /// Unix 소유자·그룹·기타의 rwx 9비트. setuid·setgid·sticky 는 적용할 때의 값을 그대로 둔다.
    Mode(u32),
    /// Windows 파일 특성 "읽기 전용". ACL 은 바꾸지 않는다.
    ReadOnly(bool),
}

/// 권한을 바꾼다. 적용하는 순간 다시 읽은 값에 9비트만 바꿔 쓰므로 그 사이 바뀐 특수 비트도 남는다.
pub(crate) fn apply(path: &Path, change: PermChange) -> Result<(), String> {
    let meta = std::fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if meta.file_type().is_symlink() {
        return Err(std::io::Error::from(std::io::ErrorKind::InvalidInput).to_string());
    }
    let mut perms = meta.permissions();
    match change {
        PermChange::Mode(bits) => set_mode(&mut perms, bits)?,
        // Unix 의 readonly 는 모든 쓰기 비트를 끄므로 Windows 특성에만 쓴다.
        PermChange::ReadOnly(read_only) => perms.set_readonly(read_only),
    }
    std::fs::set_permissions(path, perms).map_err(|e| e.to_string())
}

#[cfg(unix)]
fn set_mode(perms: &mut std::fs::Permissions, bits: u32) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let special = perms.mode() & 0o7000;
    perms.set_mode(special | (bits & 0o777));
    Ok(())
}

#[cfg(not(unix))]
fn set_mode(_perms: &mut std::fs::Permissions, _bits: u32) -> Result<(), String> {
    Err(std::io::Error::from(std::io::ErrorKind::Unsupported).to_string())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn mode(path: &Path) -> u32 {
        std::fs::symlink_metadata(path)
            .unwrap()
            .permissions()
            .mode()
            & 0o7777
    }

    #[test]
    fn the_nine_bits_change_and_special_bits_are_kept() {
        let dir = tempfile::tempdir().unwrap();
        let shared = dir.path().join("shared");
        std::fs::create_dir(&shared).unwrap();
        std::fs::set_permissions(&shared, std::fs::Permissions::from_mode(0o1775)).unwrap();
        let inner = shared.join("inner.txt");
        std::fs::write(&inner, b"x").unwrap();
        std::fs::set_permissions(&inner, std::fs::Permissions::from_mode(0o644)).unwrap();

        apply(&shared, PermChange::Mode(0o750)).unwrap();
        assert_eq!(mode(&shared), 0o1750, "sticky stays, the nine bits change");
        assert_eq!(mode(&inner), 0o644, "never recursive");
    }

    #[test]
    fn a_link_is_refused_and_its_target_is_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("t.txt");
        std::fs::write(&target, b"x").unwrap();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o644)).unwrap();
        let link = dir.path().join("l");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(apply(&link, PermChange::Mode(0o600)).is_err());
        assert_eq!(mode(&target), 0o644);
    }

    #[test]
    fn a_missing_item_reports_the_os_error() {
        let dir = tempfile::tempdir().unwrap();
        let gone = dir.path().join("gone");
        let err = apply(&gone, PermChange::Mode(0o600)).unwrap_err();
        assert_eq!(
            err,
            std::fs::symlink_metadata(&gone).unwrap_err().to_string()
        );
    }
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;

    #[test]
    fn the_read_only_attribute_turns_on_and_off() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("report.docx");
        std::fs::write(&file, b"x").unwrap();
        apply(&file, PermChange::ReadOnly(true)).unwrap();
        assert!(std::fs::metadata(&file).unwrap().permissions().readonly());
        apply(&file, PermChange::ReadOnly(false)).unwrap();
        assert!(!std::fs::metadata(&file).unwrap().permissions().readonly());
        assert!(apply(&file, PermChange::Mode(0o600)).is_err());
    }
}
