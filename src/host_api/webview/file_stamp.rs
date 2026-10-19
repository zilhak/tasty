//! 로컬 파일 한 판(version)의 식별값. 스캔한 내용과 나중에 그 경로를 여는 로드가 같은 파일 판을
//! 가리키는지 메타데이터만으로 확인한다(docs/surfaces/html/index.md). 내용을 다시 읽지 않는다.

use std::fs::Metadata;
use std::path::Path;
use std::time::SystemTime;

/// 크기·수정 시각·생성 시각과(unix) 장치·inode. 제자리 쓰기는 크기나 수정 시각을, 이름 바꾸기로 한
/// 교체는 inode나 생성 시각을 바꾼다. 파일 시스템이 이 값들을 모두 그대로 두는 쓰기는 구별하지 못한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileStamp {
    len: u64,
    modified: Option<SystemTime>,
    created: Option<SystemTime>,
    #[cfg(unix)]
    dev: u64,
    #[cfg(unix)]
    ino: u64,
}

impl FileStamp {
    pub fn of(meta: &Metadata) -> Self {
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;
        Self {
            len: meta.len(),
            modified: meta.modified().ok(),
            created: meta.created().ok(),
            #[cfg(unix)]
            dev: meta.dev(),
            #[cfg(unix)]
            ino: meta.ino(),
        }
    }

    /// 경로가 지금 가리키는 정규 파일의 식별값. 메타데이터 조회 한 번이며 내용은 읽지 않는다.
    // 이유: Windows backend와 시험만 쓴다.
    #[cfg_attr(not(any(windows, test)), allow(dead_code))]
    pub fn of_path(path: &Path) -> Option<Self> {
        let meta = std::fs::metadata(path).ok()?;
        meta.is_file().then(|| Self::of(&meta))
    }
}
