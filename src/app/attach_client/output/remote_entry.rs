//! 원격 탐색기 목록 응답 항목을 로컬 목록과 같은 `DirEntryInfo` 로 바꾼다.

use tasty_remote::client_session::{RemoteDirEntry, RemoteLink};

use crate::core::fs_list::{DirEntryInfo, EntryLink};

/// `dir` 은 원격이 실제로 읽은 폴더다. 없으면 이름만으로 경로를 만든다. OS 변경 값(`change`)은 없다.
/// 링크 상태와 OS 숨김 표시는 server 가 실은 값이고 옛 server 면 링크 아님·거짓이다. 필드를 모두 적어
/// 새 필드가 생기면 여기서 컴파일러가 알린다.
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
        link: match entry.link {
            RemoteLink::NotALink => EntryLink::NotALink,
            RemoteLink::Valid => EntryLink::Valid,
            RemoteLink::Broken => EntryLink::Broken,
        },
        os_hidden: entry.os_hidden,
        change: None,
    }
}
