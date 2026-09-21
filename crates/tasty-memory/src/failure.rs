//! SQLite 오류의 **원인 분류** — 두 DB 의 초기화와 `memory.db` 의 저장이 같은 표를 쓴다.
//!
//! 한때 이 표는 두 자리(`memory.db` 의 `MemoryInitError` 와 본 바이너리 `state.db` 의
//! `DbInitError`)에 글자 그대로 복제돼 있었고, 저장 경로에는 **아예 없었다** — 쓰기가
//! 실패하면 `rusqlite::Error` 가 그대로 올라가 호출자가 "잠겨 있었다" 와 "디스크가 찼다"
//! 와 "파일이 깨졌다" 를 고를 수 없었다. 처방이 셋 다 다르다(기다렸다 다시 · 공간을
//! 비운다 · 복구·재생성). 그래서 표를 여기 하나로 두고 초기화 오류 타입은 이것을 자기
//! variant 로 옮기기만 한다.

use rusqlite::ErrorCode;

/// SQLite 오류 하나가 어느 원인 갈래인가.
///
/// 이름(`as_str`)이 IPC 응답의 `error.data.storage_failure` 로 나가므로 **바꾸지
/// 않는다** — 소비자가 이 문자열로 분기한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageFailure {
    /// 다른 연결이 잠금을 쥐고 있었다(`SQLITE_BUSY` · `SQLITE_LOCKED`). 재시도로 풀릴 수 있다.
    Busy,
    /// 볼륨이 찼거나 DB 가 페이지 상한에 닿았다(`SQLITE_FULL`).
    DiskFull,
    /// OS 가 읽기·쓰기를 거절했다(`SQLITE_IOERR`). 디스크·파일시스템 쪽 문제다.
    Io,
    /// 파일이 SQLite DB 가 아니거나 깨졌다(`SQLITE_CORRUPT` · `SQLITE_NOTADB`).
    Corrupt,
    /// 권한 또는 열기 실패(`SQLITE_PERM` · `SQLITE_CANTOPEN`). `CANTOPEN` 은 권한·존재·
    /// 디렉터리 등 원인이 섞여 있지만 초기화 안내가 권한으로 묶어 왔으므로 그대로 둔다.
    PermissionDenied,
    /// 위 어디에도 안 드는 것(제약 위반 · SQL 오류 · SQLite 밖의 오류 등).
    Other,
}

impl StorageFailure {
    /// 오류 하나를 분류한다. SQLite 오류가 아닌 `rusqlite::Error`(변환 실패 등)는 `Other`.
    pub fn classify(err: &rusqlite::Error) -> Self {
        let rusqlite::Error::SqliteFailure(sqlite_err, _) = err else {
            return Self::Other;
        };
        match sqlite_err.code {
            ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked => Self::Busy,
            ErrorCode::DiskFull => Self::DiskFull,
            ErrorCode::SystemIoFailure => Self::Io,
            ErrorCode::DatabaseCorrupt | ErrorCode::NotADatabase => Self::Corrupt,
            ErrorCode::PermissionDenied | ErrorCode::CannotOpen => Self::PermissionDenied,
            _ => Self::Other,
        }
    }

    /// 밖으로 나가는 이름. 위 variant 와 1:1 이다.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Busy => "busy",
            Self::DiskFull => "disk_full",
            Self::Io => "io",
            Self::Corrupt => "corrupt",
            Self::PermissionDenied => "permission_denied",
            Self::Other => "other",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sqlite(code: std::ffi::c_int) -> rusqlite::Error {
        rusqlite::Error::SqliteFailure(rusqlite::ffi::Error::new(code), None)
    }

    /// 표의 각 줄이 제 갈래로 간다. 한 줄이 다른 갈래로 새면 처방이 뒤바뀐다.
    #[test]
    fn each_sqlite_code_lands_in_its_own_branch() {
        use rusqlite::ffi::*;
        let cases = [
            (SQLITE_BUSY, StorageFailure::Busy),
            (SQLITE_LOCKED, StorageFailure::Busy),
            (SQLITE_FULL, StorageFailure::DiskFull),
            (SQLITE_IOERR, StorageFailure::Io),
            (SQLITE_IOERR_WRITE, StorageFailure::Io),
            (SQLITE_IOERR_FSYNC, StorageFailure::Io),
            (SQLITE_CORRUPT, StorageFailure::Corrupt),
            (SQLITE_NOTADB, StorageFailure::Corrupt),
            (SQLITE_PERM, StorageFailure::PermissionDenied),
            (SQLITE_CANTOPEN, StorageFailure::PermissionDenied),
            (SQLITE_CONSTRAINT, StorageFailure::Other),
        ];
        for (code, want) in cases {
            assert_eq!(StorageFailure::classify(&sqlite(code)), want, "code {code}");
        }
        assert_eq!(
            StorageFailure::classify(&rusqlite::Error::InvalidQuery),
            StorageFailure::Other,
            "SQLite 밖의 오류는 원인 갈래가 없다"
        );
    }
}
