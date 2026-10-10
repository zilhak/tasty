//! 원격 탐색기 목록 응답 항목을 로컬 목록과 같은 `DirEntryInfo` 로 바꾼다.

use tasty_remote::client_session::RemoteDirEntry;

use crate::core::fs_list::DirEntryInfo;

/// `dir` 은 원격이 실제로 읽은 폴더다. 없으면 이름만으로 경로를 만든다. 원격 응답에는 링크 상태와
/// OS 숨김 표시가 없어 둘 다 기본값이다. 필드를 모두 적어 새 필드가 생기면 여기서 컴파일러가 알린다.
pub(super) fn from_remote(dir: Option<&str>, entry: RemoteDirEntry) -> DirEntryInfo {
    DirEntryInfo {
        path: dir
            .map(|dir| std::path::Path::new(dir).join(&entry.name))
            .unwrap_or_else(|| std::path::PathBuf::from(&entry.name)),
        name: entry.name,
        is_dir: entry.is_dir,
        size: entry.size,
        modified: entry.modified,
        ext: entry.ext,
        link: Default::default(),
        os_hidden: false,
    }
}
