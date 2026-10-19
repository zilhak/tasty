//! GUI의 state.db 연결. 헤드리스의 memory.db와는 수명·스키마가 다르다.
//! init 실패는 부팅 코드가 경고 알림으로 알리고, 앱은 이 DB 없이 계속 실행한다.
//! with_state_db의 None은 미초기화만 나타내므로 헤드리스와 GUI 초기화 실패를 구별할 수 없다.
//! 사용자 설정과 셸 스크립트는 이 DB에 저장하지 않는다.

#[cfg(any(feature = "gui", test))]
mod migrations;

#[cfg(any(feature = "gui", test))]
use std::io;
#[cfg(any(feature = "gui", test))]
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};

use rusqlite::Connection;

#[cfg(any(feature = "gui", test))]
pub use migrations::DbSchemaError;

pub struct Db {
    pub conn: Connection,
    /// 여러 창이 같은 DB 수명의 최근 파일 캐시를 사용한다.
    pub(crate) recent_files: Option<crate::recent_files::RecentFiles>,
    /// 연결을 열 때 기록한 pragma 요청값·확인값. 이후 변경을 실시간으로 조회하지는 않는다.
    pub(crate) applied_pragmas: tasty_memory::pragma::AppliedPragmas,
}

#[cfg(any(feature = "gui", test))]
impl Db {
    pub fn open(path: &Path) -> Result<Self, DbInitError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| classify_io(e, parent))?;
        }
        let conn = Connection::open(path).map_err(|e| classify_sql(e, path))?;
        Self::prepare(conn, path)
    }

    fn prepare(mut conn: Connection, path: &Path) -> Result<Self, DbInitError> {
        // 연결 옵션 정책은 memory.db와 공유하고 스키마 준비는 별도로 수행한다.
        let applied_pragmas = tasty_memory::pragma::apply_connection_pragmas(&conn, path);

        migrations::ensure_schema(&mut conn).map_err(|e| match e {
            DbSchemaError::SchemaMismatch { expected, found } => {
                DbInitError::SchemaMismatch { expected, found }
            }
            DbSchemaError::Sql(e) => classify_sql(e, path),
        })?;
        Ok(Self {
            conn,
            recent_files: None,
            applied_pragmas,
        })
    }
}

#[derive(Debug)]
#[cfg(any(feature = "gui", test))]
pub enum DbInitError {
    HomeDirMissing,
    PermissionDenied(PathBuf),
    Busy(PathBuf),
    DiskFull,
    Corrupt(PathBuf),
    SchemaMismatch { expected: u32, found: u32 },
    Other(String),
}

#[cfg(any(feature = "gui", test))]
impl DbInitError {
    pub fn user_message_i18n(&self) -> (&'static str, Vec<String>) {
        match self {
            DbInitError::HomeDirMissing => ("db_error.home_missing", vec![]),
            DbInitError::PermissionDenied(p) => {
                ("db_error.permission_denied", vec![p.display().to_string()])
            }
            DbInitError::Busy(p) => ("db_error.busy", vec![p.display().to_string()]),
            DbInitError::DiskFull => ("db_error.disk_full", vec![]),
            DbInitError::Corrupt(p) => ("db_error.corrupt", vec![p.display().to_string()]),
            DbInitError::SchemaMismatch { expected, found } => (
                "db_error.schema_mismatch",
                vec![expected.to_string(), found.to_string()],
            ),
            DbInitError::Other(msg) => ("db_error.other", vec![msg.clone()]),
        }
    }
}

#[cfg(any(feature = "gui", test))]
impl std::fmt::Display for DbInitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DbInitError::HomeDirMissing => write!(f, "home directory missing"),
            DbInitError::PermissionDenied(p) => {
                write!(f, "permission denied: {}", p.display())
            }
            DbInitError::Busy(p) => write!(f, "database busy: {}", p.display()),
            DbInitError::DiskFull => write!(f, "disk full"),
            DbInitError::Corrupt(p) => write!(f, "database corrupted: {}", p.display()),
            DbInitError::SchemaMismatch { expected, found } => {
                write!(f, "schema mismatch (expected {expected}, found {found})")
            }
            DbInitError::Other(msg) => write!(f, "{msg}"),
        }
    }
}

#[cfg(any(feature = "gui", test))]
impl std::error::Error for DbInitError {}

#[cfg(any(feature = "gui", test))]
fn classify_io(err: io::Error, path: &Path) -> DbInitError {
    match err.kind() {
        io::ErrorKind::PermissionDenied => DbInitError::PermissionDenied(path.to_path_buf()),
        _ if err.raw_os_error() == Some(disk_full_os_error()) => DbInitError::DiskFull,
        _ => DbInitError::Other(format!("{path:?}: {err}", path = path.display())),
    }
}

/// 디스크 용량 부족의 OS 오류 코드. 다른 파일 저장 오류 분류에서도 사용한다.
#[cfg(unix)]
#[cfg(any(feature = "gui", test))]
pub(crate) fn disk_full_os_error() -> i32 {
    28 // ENOSPC
}

#[cfg(windows)]
#[cfg(any(feature = "gui", test))]
pub(crate) fn disk_full_os_error() -> i32 {
    112 // ERROR_DISK_FULL
}

/// memory.db와 같은 SQLite 오류 분류를 사용한다. 일반 I/O 오류는 Other로 안내한다.
#[cfg(any(feature = "gui", test))]
fn classify_sql(err: rusqlite::Error, path: &Path) -> DbInitError {
    use tasty_memory::StorageFailure;
    match StorageFailure::classify(&err) {
        StorageFailure::Busy => DbInitError::Busy(path.to_path_buf()),
        StorageFailure::Corrupt => DbInitError::Corrupt(path.to_path_buf()),
        StorageFailure::DiskFull => DbInitError::DiskFull,
        StorageFailure::PermissionDenied => DbInitError::PermissionDenied(path.to_path_buf()),
        StorageFailure::Io | StorageFailure::Other => {
            DbInitError::Other(format!("{}: {err}", path.display()))
        }
    }
}

static DB: OnceLock<Mutex<Db>> = OnceLock::new();

static DB_POISONED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
const DB_WHAT: &str = "state.db connection";

#[cfg(feature = "gui")]
pub fn default_db_path() -> Option<PathBuf> {
    tasty_utils::path::tasty_home().map(|d| d.join("state.db"))
}

/// 이번 실행에서 초기화를 이미 시도했는지. 실패한 뒤 창마다 다시 열어 안내가 쌓이지 않게 한다.
#[cfg(feature = "gui")]
static INIT_ATTEMPTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// GUI 부팅에서 실행당 한 번 초기화한다. 처음 실패했을 때만 원인을 돌려주며 안내는 호출자가 맡는다.
/// 실패한 뒤에는 같은 실행에서 다시 열지 않고, 소비자는 `with_state_db`의 None으로 기본값을 쓴다.
/// 원본 파일은 열기만 시도하며 지우거나 새로 만들어 덮지 않는다.
#[cfg(feature = "gui")]
pub fn init() -> Option<DbInitError> {
    init_once(&INIT_ATTEMPTED, &DB, || {
        let path = default_db_path().ok_or(DbInitError::HomeDirMissing)?;
        Db::open(&path).map(|db| (path, db))
    })
}

#[cfg(any(feature = "gui", test))]
fn init_once(
    attempted: &std::sync::atomic::AtomicBool,
    slot: &OnceLock<Mutex<Db>>,
    open: impl FnOnce() -> Result<(PathBuf, Db), DbInitError>,
) -> Option<DbInitError> {
    use std::sync::atomic::Ordering;
    if attempted.swap(true, Ordering::AcqRel) {
        return None;
    }
    match open() {
        Ok((path, db)) => {
            tracing::info!("opened state.db at {}", path.display());
            slot.get_or_init(|| Mutex::new(db));
            None
        }
        Err(error) => {
            tracing::warn!(
                "state.db init failed: {error} — continuing without recent files and tutorial progress"
            );
            Some(error)
        }
    }
}

/// 초기화되지 않았으면 None이다. poison은 로그 후 기존 연결로 복구하며 None으로 숨기지 않는다.
/// 이 접근자가 트랜잭션·캐시의 논리적 정합을 다시 검증하는 것은 아니다.
pub fn with_state_db<T>(f: impl FnOnce(&mut Db) -> T) -> Option<T> {
    let mutex = DB.get()?;
    let mut guard: MutexGuard<'_, Db> =
        crate::poison::recover_mutex(mutex.lock(), DB_WHAT, &DB_POISONED);
    Some(f(&mut guard))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Db::prepare가 공용 pragma 설정을 실제로 적용하는지 확인한다.
    #[test]
    fn journal_size_limit_matches_the_memory_store() {
        let tmp = tempfile::tempdir().unwrap();
        let db = Db::open(&tmp.path().join("state.db")).unwrap();
        let limit: i64 = db
            .conn
            .query_row("PRAGMA journal_size_limit", [], |r| r.get(0))
            .unwrap();
        assert_eq!(limit, tasty_memory::WAL_SIZE_LIMIT_BYTES);
    }

    /// 손상 파일을 열지 못해도 원본 바이트를 바꾸지 않고, 같은 실행에서는 다시 열지 않는다.
    #[test]
    fn a_failed_open_is_reported_once_and_leaves_the_file_untouched() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("state.db");
        let junk = b"not a sqlite database, just bytes".repeat(64);
        std::fs::write(&path, &junk).unwrap();
        let attempted = std::sync::atomic::AtomicBool::new(false);
        let slot = OnceLock::new();
        let open = || Db::open(&path).map(|db| (path.clone(), db));
        let first = init_once(&attempted, &slot, open);
        assert!(matches!(first, Some(DbInitError::Corrupt(_))), "{first:?}");
        let mut calls = 0;
        let second = init_once(&attempted, &slot, || {
            calls += 1;
            Db::open(&path).map(|db| (path.clone(), db))
        });
        assert!(second.is_none());
        assert_eq!(calls, 0, "the same run must not reopen after a failure");
        assert!(slot.get().is_none());
        assert_eq!(std::fs::read(&path).unwrap(), junk);
    }

    #[test]
    fn classify_sql_busy() {
        let err = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY),
            None,
        );
        let classified = classify_sql(err, Path::new("/tmp/x.db"));
        assert!(matches!(classified, DbInitError::Busy(_)));
    }

    #[test]
    fn classify_sql_corrupt() {
        let err = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CORRUPT),
            None,
        );
        let classified = classify_sql(err, Path::new("/tmp/x.db"));
        assert!(matches!(classified, DbInitError::Corrupt(_)));
    }

    #[test]
    fn classify_sql_notadb_is_corrupt() {
        let err = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_NOTADB),
            None,
        );
        let classified = classify_sql(err, Path::new("/tmp/x.db"));
        assert!(matches!(classified, DbInitError::Corrupt(_)));
    }

    #[test]
    fn user_message_keys_are_stable() {
        let cases: &[(DbInitError, &str)] = &[
            (DbInitError::HomeDirMissing, "db_error.home_missing"),
            (
                DbInitError::PermissionDenied(PathBuf::from("/a")),
                "db_error.permission_denied",
            ),
            (DbInitError::Busy(PathBuf::from("/a")), "db_error.busy"),
            (DbInitError::DiskFull, "db_error.disk_full"),
            (
                DbInitError::Corrupt(PathBuf::from("/a")),
                "db_error.corrupt",
            ),
            (
                DbInitError::SchemaMismatch {
                    expected: 1,
                    found: 2,
                },
                "db_error.schema_mismatch",
            ),
            (DbInitError::Other("x".into()), "db_error.other"),
        ];
        for (err, expected_key) in cases {
            let (key, _args) = err.user_message_i18n();
            assert_eq!(key, *expected_key, "mismatch for {err:?}");
        }
    }
}
