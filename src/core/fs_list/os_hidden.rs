//! 목록 항목의 OS 숨김 표시. Windows 는 파일 속성의 `FILE_ATTRIBUTE_HIDDEN`, macOS 는 BSD 파일
//! 플래그의 `UF_HIDDEN`(`chflags hidden`)을 본다. 그 밖의 OS 에는 이런 표시가 없어 늘 거짓이다.
//! 이름 앞 점 규칙은 여기서 보지 않는다. 탐색기의 숨김 판정이 두 가지를 함께 본다.
//! 링크는 대상이 아니라 링크 자신의 표시를 본다(`DirEntry::metadata` 는 링크를 따라가지 않는다).
//! 호출자가 항목 자신의 metadata 를 이미 읽었으면 `own` 으로 넘겨 다시 읽지 않는다.

use std::fs::{DirEntry, Metadata};

/// Windows `FILE_ATTRIBUTE_HIDDEN`.
#[cfg(any(windows, test))]
const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
/// macOS `<sys/stat.h>` 의 `UF_HIDDEN`.
#[cfg(any(target_os = "macos", test))]
const UF_HIDDEN: u32 = 0x8000;

#[cfg(windows)]
pub(super) fn is_os_hidden(entry: &DirEntry, own: Option<&Metadata>) -> bool {
    use std::os::windows::fs::MetadataExt;
    let marked = |m: &Metadata| attributes_hidden(m.file_attributes());
    own.map_or_else(|| entry.metadata().is_ok_and(|m| marked(&m)), marked)
}

#[cfg(target_os = "macos")]
pub(super) fn is_os_hidden(entry: &DirEntry, own: Option<&Metadata>) -> bool {
    use std::os::macos::fs::MetadataExt;
    let marked = |m: &Metadata| flags_hidden(m.st_flags());
    own.map_or_else(|| entry.metadata().is_ok_and(|m| marked(&m)), marked)
}

#[cfg(not(any(windows, target_os = "macos")))]
pub(super) fn is_os_hidden(_entry: &DirEntry, _own: Option<&Metadata>) -> bool {
    false
}

/// 파일 속성 비트에 숨김 비트가 있는가. 다른 속성(보관·시스템 등)은 보지 않는다.
#[cfg(any(windows, test))]
fn attributes_hidden(attributes: u32) -> bool {
    attributes & FILE_ATTRIBUTE_HIDDEN != 0
}

/// BSD 파일 플래그에 `UF_HIDDEN` 이 있는가. 다른 플래그(nodump·immutable 등)는 보지 않는다.
#[cfg(any(target_os = "macos", test))]
fn flags_hidden(flags: u32) -> bool {
    flags & UF_HIDDEN != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_hidden_attribute_bit_counts_on_windows() {
        assert!(attributes_hidden(0x2));
        // 숨김 + 보관(0x20) + 시스템(0x4) 처럼 다른 비트가 함께 있어도 숨김이다.
        assert!(attributes_hidden(0x2 | 0x4 | 0x20));
        assert!(!attributes_hidden(0x20));
        assert!(!attributes_hidden(0x4));
        assert!(!attributes_hidden(0x10)); // 디렉터리 비트만
        assert!(!attributes_hidden(0));
    }

    #[test]
    fn only_the_uf_hidden_flag_counts_on_macos() {
        assert!(flags_hidden(0x8000));
        // UF_NODUMP(0x1) 처럼 다른 플래그가 함께 있어도 숨김이다.
        assert!(flags_hidden(0x8000 | 0x1));
        assert!(!flags_hidden(0x1));
        assert!(!flags_hidden(0x2)); // UF_IMMUTABLE
        assert!(!flags_hidden(0));
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    #[test]
    fn other_systems_have_no_os_hidden_mark() {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::write(dir.path().join("plain"), b"").expect("write");
        let entries: Vec<_> = std::fs::read_dir(dir.path())
            .expect("read dir")
            .filter_map(Result::ok)
            .collect();
        assert_eq!(entries.len(), 1);
        assert!(!is_os_hidden(&entries[0], None));
    }

    /// 목록이 돌려준 (이름, OS 숨김) 을 이름 순으로.
    #[cfg(any(windows, target_os = "macos"))]
    fn listed(dir: &std::path::Path) -> Vec<(String, bool)> {
        let mut v: Vec<(String, bool)> = super::super::read_dir_entries(dir)
            .expect("list")
            .into_iter()
            .map(|e| (e.name, e.os_hidden))
            .collect();
        v.sort();
        v
    }

    /// Windows CI 에서만 돈다. 숨김 속성으로 만든 파일이 목록에서 OS 숨김으로 나온다.
    #[cfg(windows)]
    #[test]
    fn a_file_created_with_the_hidden_attribute_is_listed_as_os_hidden() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .attributes(FILE_ATTRIBUTE_HIDDEN)
            .open(dir.path().join("marked.txt"))
            .expect("create hidden file");
        std::fs::write(dir.path().join("plain.txt"), b"").expect("write");
        assert_eq!(
            listed(dir.path()),
            [("marked.txt".into(), true), ("plain.txt".into(), false)]
        );
    }

    /// macOS CI 에서만 돈다. `chflags hidden` 한 파일이 목록에서 OS 숨김으로 나오고, 그 파일을
    /// 가리키는 링크는 링크 자신의 플래그(없음)를 따른다.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_file_flagged_hidden_is_listed_as_os_hidden_but_its_link_is_not() {
        let dir = tempfile::tempdir().expect("temp dir");
        let marked = dir.path().join("marked.txt");
        std::fs::write(&marked, b"").expect("write");
        std::fs::write(dir.path().join("plain.txt"), b"").expect("write");
        std::os::unix::fs::symlink(&marked, dir.path().join("to-marked")).expect("symlink");
        let status = std::process::Command::new("chflags")
            .arg("hidden")
            .arg(&marked)
            .status()
            .expect("run chflags");
        assert!(status.success(), "chflags hidden failed: {status}");
        assert_eq!(
            listed(dir.path()),
            [
                ("marked.txt".into(), true),
                ("plain.txt".into(), false),
                ("to-marked".into(), false),
            ]
        );
    }
}
