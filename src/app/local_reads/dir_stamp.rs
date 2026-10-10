//! 탐색기의 외부 변경 확인에 쓰는 폴더 표지. 폴더 metadata 만 읽고 항목은 읽지 않는다.
//! 목록과 함께 읽을 때는 표지를 먼저 읽는다. 그 사이에 바뀌면 다음 확인에서 다시 읽게 되어
//! 변경을 놓치지 않는다. 규칙: docs/surfaces/explorer/index.md#외부-변경-확인.

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

/// 목록과 그 목록을 읽기 직전의 폴더 표지.
pub(crate) struct StampedListing {
    pub(crate) stamp: Option<DirStamp>,
    pub(crate) entries: Vec<DirEntryInfo>,
}

pub(super) fn read_stamped(
    dir: &Path,
    read: impl FnOnce(&Path) -> io::Result<Vec<DirEntryInfo>>,
) -> io::Result<StampedListing> {
    let stamp = DirStamp::read(dir);
    read(dir).map(|entries| StampedListing { stamp, entries })
}

pub(super) fn read_stamps(dirs: Vec<PathBuf>) -> Vec<(PathBuf, Option<DirStamp>)> {
    dirs.into_iter()
        .map(|dir| {
            let stamp = DirStamp::read(&dir);
            (dir, stamp)
        })
        .collect()
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
}
