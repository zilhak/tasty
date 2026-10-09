//! 다른 디스크로 옮긴 뒤 남은 원본을 다시 지운다. 사본이 공개한 자리에 같은 종류로 남아 있을 때만
//! 지우고, 사본이 없거나 바뀌었거나 원본 파일이 그 뒤 바뀌었으면 원본을 남기고 사유를 적는다.
//! 폴더 원본은 안쪽을 걸어 사본에 같은 상대 경로·종류·크기(링크는 대상)로 있는 항목만 지운다.
//! 이동 뒤 넣거나 바꾼 항목은 사본에 없으므로 남는다. 지우기는 처음 이동과 같은 영구 삭제다.
//! 되돌리기 단계는 만들지 않는다.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::SystemTime;

use super::super::ops::remove_path;
use super::{Failure, OpKind, Reason, Report, Shared};

/// 경로가 가리키는 항목의 종류. 링크는 따라가지 않는다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EntryKind {
    File,
    Dir,
    Link,
}

impl EntryKind {
    fn of(path: &Path) -> Option<Self> {
        let meta = path.symlink_metadata().ok()?;
        let kind = meta.file_type();
        Some(if kind.is_symlink() {
            Self::Link
        } else if kind.is_dir() {
            Self::Dir
        } else {
            Self::File
        })
    }
}

/// 원본 파일을 지우려던 때의 모습. 다시 지울 때 달라졌으면 그 뒤 고친 원본으로 보고 남긴다.
#[derive(Clone, Debug, PartialEq, Eq)]
struct FileStamp {
    modified: Option<SystemTime>,
    len: u64,
}

impl FileStamp {
    fn read(path: &Path) -> Option<Self> {
        let meta = path.symlink_metadata().ok()?;
        meta.is_file().then(|| Self {
            modified: meta.modified().ok(),
            len: meta.len(),
        })
    }
}

/// 사본은 공개했지만 원본을 다 지우지 못한 이동 항목.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Leftover {
    /// 남은 원본의 경로.
    pub source: PathBuf,
    /// 공개한 사본의 경로.
    pub copy: PathBuf,
    /// 공개한 사본의 종류. 지울 수 없는 경로면 None 이고 다시 지우지 않는다.
    copy_kind: Option<EntryKind>,
    /// 원본이 파일이었으면 지우려던 때의 모습. 폴더는 안쪽 변경을 알 수 없어 None 이다.
    source_file: Option<FileStamp>,
}

impl Leftover {
    /// 사본을 공개한 직후, 원본을 지우기 전에 기록한다.
    pub(crate) fn before_remove(source: &Path, copy: &Path) -> Self {
        Self {
            source: source.to_path_buf(),
            copy: copy.to_path_buf(),
            copy_kind: EntryKind::of(copy),
            source_file: FileStamp::read(source),
        }
    }

    /// 원본을 지워도 되는지 본다. 지울 원본이 이미 없으면 Ok(false).
    fn check(&self) -> Result<bool, Reason> {
        if self.copy_kind.is_none() || EntryKind::of(&self.copy) != self.copy_kind {
            return Err(Reason::CopyMissing);
        }
        if self.source.symlink_metadata().is_err() {
            return Ok(false);
        }
        if self.source_file.is_some() && FileStamp::read(&self.source) != self.source_file {
            return Err(Reason::ChangedSince);
        }
        if self.copy_kind == Some(EntryKind::Link) && !same_entry(&self.source, &self.copy) {
            return Err(Reason::ChangedSince);
        }
        Ok(true)
    }

    /// 확인을 통과한 원본을 지운다. 폴더는 사본과 같은 항목만 지우고 남긴 수를 알린다.
    fn remove(&self) -> Result<(), Reason> {
        if self.copy_kind != Some(EntryKind::Dir) {
            return remove_path(&self.source).map_err(|e| Reason::SourceNotRemoved(e.to_string()));
        }
        let mut pruned = Pruned::default();
        prune_dir(&self.source, &self.copy, &mut pruned);
        match (pruned.error, pruned.kept) {
            (Some(e), _) => Err(Reason::SourceNotRemoved(e.to_string())),
            (None, 0) => Ok(()),
            (None, kept) => Err(Reason::KeptNotInCopy(kept)),
        }
    }
}

/// 폴더 원본을 걸며 모은 결과.
#[derive(Default)]
struct Pruned {
    /// 사본에 같은 항목이 없어 남긴 항목 수. 남긴 폴더는 그 안을 세지 않고 하나로 센다.
    kept: usize,
    /// 지우려다 실패한 첫 오류. 나머지 항목은 계속 본다.
    error: Option<io::Error>,
}

/// 원본 항목과 사본 항목이 같은가. 링크는 따라가지 않고 링크 자체(대상 경로)를 비교하고,
/// 파일은 크기를 비교한다. 폴더는 종류만 본다(안쪽은 [`prune_dir`] 가 항목별로 본다).
fn same_entry(source: &Path, copy: &Path) -> bool {
    let (Ok(s), Ok(c)) = (source.symlink_metadata(), copy.symlink_metadata()) else {
        return false;
    };
    let (st, ct) = (s.file_type(), c.file_type());
    if st.is_symlink() || ct.is_symlink() {
        return st.is_symlink()
            && ct.is_symlink()
            && matches!(
                (std::fs::read_link(source), std::fs::read_link(copy)),
                (Ok(a), Ok(b)) if a == b
            );
    }
    if st.is_dir() || ct.is_dir() {
        return st.is_dir() && ct.is_dir();
    }
    st.is_file() && ct.is_file() && s.len() == c.len()
}

/// 폴더 원본 `source` 에서 사본 `copy` 에 같은 상대 경로로 같은 항목이 있는 것만 지운다.
/// 다 지워 비면 폴더도 지운다. 읽지 못한 폴더는 남긴 것으로 센다.
fn prune_dir(source: &Path, copy: &Path, pruned: &mut Pruned) {
    let entries = match std::fs::read_dir(source) {
        Ok(entries) => entries,
        Err(e) => {
            pruned.error.get_or_insert(e);
            return;
        }
    };
    let mut left = false;
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(e) => {
                left = true;
                pruned.error.get_or_insert(e);
                continue;
            }
        };
        let path = entry.path();
        let twin = copy.join(entry.file_name());
        if !same_entry(&path, &twin) {
            left = true;
            pruned.kept += 1;
            continue;
        }
        let is_dir = path.symlink_metadata().is_ok_and(|m| m.is_dir());
        if is_dir {
            prune_dir(&path, &twin, pruned);
            left |= path.symlink_metadata().is_ok();
        } else if let Err(e) = std::fs::remove_file(&path) {
            left = true;
            pruned.error.get_or_insert(e);
        }
    }
    if !left && let Err(e) = std::fs::remove_dir(source) {
        pruned.error.get_or_insert(e);
    }
}

/// 남은 원본들을 다시 지운다. 결과는 처음 이동과 같은 이동 보고로 낸다.
/// 또 지우지 못한 원본은 다시 [`Report::leftovers`] 에 남아 한 번 더 시도할 수 있다.
pub(crate) fn run_remove_leftovers(
    shared: &Shared,
    dest: Option<PathBuf>,
    leftovers: &[Leftover],
) -> Report {
    let mut report = Report::new(OpKind::Move, dest, leftovers.len());
    shared.items_total.store(leftovers.len(), Ordering::Release);
    for leftover in leftovers {
        if shared.cancelled() {
            report.cancelled = true;
            break;
        }
        shared.set_current(&leftover.source);
        let result = leftover
            .check()
            .and_then(|present| if present { leftover.remove() } else { Ok(()) });
        match result {
            Ok(()) => report.done += 1,
            Err(reason) => {
                if reason.leaves_original() {
                    // 사본은 그대로라 처음 이동과 같이 끝난 항목으로 센다.
                    report.done += 1;
                    report.leftovers.push(leftover.clone());
                }
                report.failed.push(Failure {
                    path: leftover.source.clone(),
                    reason,
                });
            }
        }
        shared.finish_item(0);
    }
    report
}

#[cfg(test)]
#[path = "leftover_tests.rs"]
mod tests;
