//! 탐색기의 외부 변경 확인에 쓰는 폴더 표지. 폴더 metadata 만 읽고 항목은 읽지 않는다.
//! 목록과 함께 읽을 때는 표지를 먼저 읽는다. 그 사이에 바뀌면 다음 확인에서 다시 읽게 되어
//! 변경을 놓치지 않는다. 설정으로 켜는 파일 정보 확인은 따로 항목 서명(`entries_key`)을 비교한다.
//! 규칙: docs/surfaces/explorer/index.md#외부-변경-확인.

use std::hash::{Hash, Hasher};
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::core::fs_list::DirEntryInfo;

/// 폴더의 수정 시각과 크기. 항목이 생기거나 사라지거나 이름이 바뀌면 수정 시각이 바뀐다.
/// 시각 해상도가 거친 파일시스템에서는 같은 단위 안의 두 번째 변경을 구별하지 못한다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DirStamp {
    modified: Option<SystemTime>,
    len: u64,
}

impl DirStamp {
    /// 읽지 못하면(사라짐·권한) `None`. 그 상태가 이어지는 동안은 변경이 아니다.
    pub(crate) fn read(dir: &Path) -> Option<Self> {
        let meta = std::fs::metadata(dir).ok()?;
        Some(Self {
            modified: meta.modified().ok(),
            len: meta.len(),
        })
    }
}

/// 목록과 그 목록을 읽기 직전의 폴더 표지, 그리고 목록의 항목 서명.
pub(crate) struct StampedListing {
    pub(crate) stamp: Option<DirStamp>,
    pub(crate) entries: Vec<DirEntryInfo>,
    pub(crate) entries_key: u64,
}

pub(super) fn read_stamped(
    dir: &Path,
    read: impl FnOnce(&Path) -> io::Result<Vec<DirEntryInfo>>,
) -> io::Result<StampedListing> {
    let stamp = DirStamp::read(dir);
    read(dir).map(|entries| StampedListing {
        stamp,
        entries_key: entries_key(&entries),
        entries,
    })
}

/// 항목들의 경로와 content key(수정 시각·크기·Unix inode·ctime)를 합친 서명. 순서와 무관하다.
/// 한 항목의 내용·크기·수정 시각(Unix 는 권한 변경의 ctime 도)이 바뀌면 달라진다.
pub(crate) fn entries_key(entries: &[DirEntryInfo]) -> u64 {
    let mut keyed: Vec<_> = entries
        .iter()
        .map(|e| (e.path.as_path(), e.content_key()))
        .collect();
    keyed.sort_unstable_by(|a, b| a.0.cmp(b.0));
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    keyed.hash(&mut hasher);
    hasher.finish()
}

/// 한 회차 확인의 결과. `entries` 는 파일 정보 확인을 켰을 때 현재 폴더의 항목 서명이며,
/// 목록을 읽지 못하면 서명이 없다(사라진 폴더는 표지가 알린다).
pub(crate) struct StampCheck {
    pub(crate) stamps: Vec<(PathBuf, Option<DirStamp>)>,
    pub(crate) entries: Option<(PathBuf, Option<u64>)>,
}

/// 표지를 먼저 모두 읽고, 켜져 있으면 현재 폴더 항목을 읽는다. 한 worker 에서 차례로 읽는다.
pub(super) fn read_check(
    dirs: Vec<PathBuf>,
    entries_of: Option<PathBuf>,
    read: impl FnOnce(&Path) -> io::Result<Vec<DirEntryInfo>>,
) -> StampCheck {
    let stamps = dirs
        .into_iter()
        .map(|dir| {
            let stamp = DirStamp::read(&dir);
            (dir, stamp)
        })
        .collect();
    let entries = entries_of.map(|dir| {
        let key = read(&dir).ok().map(|entries| entries_key(&entries));
        (dir, key)
    });
    StampCheck { stamps, entries }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adding_an_entry_changes_the_folder_stamp_and_a_missing_folder_has_none() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let before = DirStamp::read(tmp.path()).expect("stamp");
        assert_eq!(
            DirStamp::read(tmp.path()),
            Some(before),
            "읽기만으로는 바뀌지 않는다"
        );
        // 임시 폴더의 파일시스템은 시각 해상도가 충분하다고 본다. 거친 해상도는 문서의 한계다.
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::create_dir(tmp.path().join("new")).expect("create");
        assert_ne!(DirStamp::read(tmp.path()), Some(before));
        assert_eq!(DirStamp::read(&tmp.path().join("gone")), None);
    }

    fn listed(dir: &Path) -> u64 {
        entries_key(&crate::core::fs_list::read_dir_entries(dir).expect("list"))
    }

    /// 파일 내용만 바뀌면 폴더 표지는 그대로지만 항목 서명은 달라진다. 다시 읽기만으로는 같다.
    #[test]
    fn a_content_change_keeps_the_folder_stamp_but_changes_the_entries_key() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let file = tmp.path().join("file");
        std::fs::write(&file, b"x").expect("write");
        let stamp = DirStamp::read(tmp.path()).expect("stamp");
        let key = listed(tmp.path());
        assert_eq!(listed(tmp.path()), key, "읽기만으로는 바뀌지 않는다");
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(&file, b"longer").expect("rewrite");
        assert_eq!(DirStamp::read(tmp.path()), Some(stamp));
        assert_ne!(listed(tmp.path()), key);
    }

    #[test]
    fn the_check_reads_entries_only_when_asked() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let dir = tmp.path().to_path_buf();
        let off = read_check(vec![dir.clone()], None, |_| panic!("항목을 읽지 않는다"));
        assert_eq!(off.stamps.len(), 1);
        assert!(off.entries.is_none());
        let on = read_check(vec![dir.clone()], Some(dir.clone()), |d| {
            crate::core::fs_list::read_dir_entries(d)
        });
        assert_eq!(on.entries, Some((dir.clone(), Some(listed(&dir)))));
        let gone = read_check(Vec::new(), Some(dir.join("gone")), |d| {
            crate::core::fs_list::read_dir_entries(d)
        });
        assert_eq!(gone.entries, Some((dir.join("gone"), None)));
    }

    /// 권한만 바꾸면 ctime 만 바뀌므로 표지는 그대로다. 문서의 한계 절이 이 동작을 적는다.
    #[cfg(unix)]
    #[test]
    fn a_permission_change_alone_leaves_the_folder_stamp() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().expect("temp dir");
        let file = tmp.path().join("file");
        std::fs::write(&file, b"x").expect("write");
        let before = DirStamp::read(tmp.path()).expect("stamp");
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600))
            .expect("chmod file");
        std::fs::set_permissions(tmp.path(), std::fs::Permissions::from_mode(0o750))
            .expect("chmod folder");
        assert_eq!(DirStamp::read(tmp.path()), Some(before));
    }
}
