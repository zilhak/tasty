//! 이름 변경과 새 항목. 성공해 무엇이 바뀌었으면 이력이 쓸 되돌리기 단계를 낸다.
//!
//! 이름 변경은 원래 이름으로 되돌리는 이동([`UndoStep::Moved`])이라, 원래 이름이 비어 있고 항목의
//! 수정 시각이 그대로일 때만 되돌린다. 새 항목은 [`UndoStep::Made`] 라, 되돌리면 휴지통으로 보내되
//! 아직 비어 있고(파일 길이 0, 빈 폴더) 만든 직후 적은 수정 시각 그대로일 때만 보낸다. 빈 항목이라는
//! 조건은 만들기와 시각 기록 사이에 다른 프로그램이 쓴 경우와 수정 시각 단위가 거친 파일시스템에서
//! 같은 시각 칸 안에 쓴 경우를 막는다. 판정과 휴지통 이동 사이의 변경은 막지 못하지만 휴지통에서
//! 되살릴 수 있다(복사 되돌리기와 같다).

use std::path::Path;
use std::time::SystemTime;

use super::job::leftover::same_file_id;
use super::job::{Reason, UndoStep, modified_at};
use super::ops;

/// 항목의 이름을 바꾼다. 같은 이름이면 아무것도 하지 않고 단계도 없다.
pub(super) fn rename(path: &Path, name: &str) -> Result<Option<UndoStep>, String> {
    ops::rename_entry(path, name).map_err(|e| e.to_string())?;
    let Some(parent) = path.parent() else {
        return Ok(None);
    };
    let to = parent.join(name);
    if to == path {
        return Ok(None);
    }
    let made = modified_at(&to);
    Ok(Some(UndoStep::Moved {
        from: path.to_path_buf(),
        to,
        made,
    }))
}

/// 새 폴더나 빈 파일을 만든다.
pub(super) fn create(dir: &Path, name: &str, folder: bool) -> Result<Option<UndoStep>, String> {
    ops::create_entry(dir, name, folder).map_err(|e| e.to_string())?;
    let path = dir.join(name);
    let made = modified_at(&path);
    Ok(Some(UndoStep::Made(path, made)))
}

/// 되돌릴 원래 자리 `from` 을 다른 항목이 차지했는가. 대소문자를 무시하는 파일시스템에서 대소문자만
/// 바꾼 이름 변경은 원래 이름이 옮긴 항목 `to` 자신을 가리키므로 비어 있는 것으로 본다. 이름이
/// 대소문자만 다르고 같은 파일(Unix dev·inode, Windows 볼륨·파일 번호)일 때만 그렇게 본다.
pub(super) fn taken(from: &Path, to: &Path) -> bool {
    from.symlink_metadata().is_ok() && !(case_only(from, to) && same_file_id(from, to))
}

fn case_only(a: &Path, b: &Path) -> bool {
    let (Some(x), Some(y)) = (a.file_name(), b.file_name()) else {
        return false;
    };
    a.parent() == b.parent()
        && x != y
        && x.to_string_lossy().to_lowercase() == y.to_string_lossy().to_lowercase()
}

/// 새로 만든 항목을 지금 되돌릴 수 없는 이유. 되돌릴 수 있으면 None.
pub(super) fn made_blocked(path: &Path, made: Option<SystemTime>) -> Option<Reason> {
    let Ok(meta) = path.symlink_metadata() else {
        return Some(Reason::Gone);
    };
    let empty = if meta.is_dir() {
        std::fs::read_dir(path).is_ok_and(|mut entries| entries.next().is_none())
    } else {
        meta.is_file() && meta.len() == 0
    };
    (!empty || meta.modified().ok() != made).then_some(Reason::ChangedSince)
}
