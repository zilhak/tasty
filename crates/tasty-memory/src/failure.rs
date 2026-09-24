//! SQLite 초기화와 저장 오류를 같은 원인 분류로 변환한다.

use rusqlite::ErrorCode;

/// SQLite 오류 분류. as_str의 값은 IPC error.data.storage_failure에 쓰이므로 호환성을 유지한다.
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
    /// 권한 또는 열기 실패(SQLITE_PERM·SQLITE_CANTOPEN). CANTOPEN에는 경로 부재 등도 포함된다.
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
