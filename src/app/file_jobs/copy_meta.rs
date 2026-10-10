//! 사본에 옮기는 원본 metadata. 파일은 권한과 수정 시각, 폴더는 권한을 옮긴다. 소유자·확장 속성·ACL 은
//! 옮기지 않는다. 링크는 링크 자신으로 다시 만들 뿐 metadata 를 옮기지 않는다.

use std::fs::{File, Metadata, Permissions};
use std::io;
use std::path::{Path, PathBuf};

/// 내용을 다 쓴 사본 파일에 원본의 권한과 수정 시각을 옮긴다. 쓰기가 수정 시각을 바꾸므로 쓴 뒤에 부른다.
/// 수정 시각은 열린 핸들로 바꾸므로 읽기 전용 권한을 먼저 걸어도 된다. 수정 시각을 바꾸지 못하는
/// 파일시스템에서는 내용이 이미 옮겨졌으므로 작업을 실패시키지 않고 경고만 남긴다.
pub(super) fn keep_file_meta(output: &File, dst: &Path, meta: &Metadata) -> io::Result<()> {
    output.set_permissions(meta.permissions())?;
    if let Ok(modified) = meta.modified()
        && let Err(error) = output.set_modified(modified)
    {
        tracing::warn!(%error, path = %dst.display(), "explorer copy: modified time not kept");
    }
    Ok(())
}

/// 사본 폴더에 걸 권한. 읽기 전용 폴더에는 안쪽 항목을 만들 수 없고, 다른 부모로 옮기는 rename 도
/// 막힌다(`..` 를 고쳐야 해서). 그래서 사본을 다 만들고 제자리로 공개한 뒤에 한꺼번에 건다.
#[derive(Default)]
pub(super) struct FolderModes(Vec<(PathBuf, Permissions)>);

impl FolderModes {
    /// 안쪽을 다 만든 폴더를 적는다. 안쪽 폴더가 바깥 폴더보다 먼저 들어온다.
    pub(super) fn push(&mut self, dst: &Path, meta: &Metadata) {
        self.0.push((dst.to_path_buf(), meta.permissions()));
    }

    /// `staged` 에서 만든 사본을 `published` 로 공개한 뒤 적은 권한을 건다. 안쪽부터 건다.
    /// 걸지 못한 폴더는 사본이 이미 공개됐으므로 작업을 되돌리지 않고 경고만 남긴다.
    pub(super) fn apply(self, staged: &Path, published: &Path) {
        for (path, perms) in self.0 {
            let Ok(rest) = path.strip_prefix(staged) else {
                continue;
            };
            let target = published.join(rest);
            if let Err(error) = std::fs::set_permissions(&target, perms) {
                tracing::warn!(%error, path = %target.display(), "explorer copy: folder permissions not kept");
            }
        }
    }
}

#[cfg(test)]
#[path = "copy_meta_tests.rs"]
mod tests;
