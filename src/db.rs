//! GUI의 state.db 연결. 헤드리스의 memory.db와는 수명·스키마가 다르다.
//! 부팅이 연 연결은 state writer(`writer` 모듈)가 소유하며 다른 코드는 연결에 닿지 않는다.
//! init 실패는 부팅 코드가 경고 알림으로 알리고, 앱은 이 DB 없이 계속 실행한다.
//! 공유 핸들이 없다는 것만으로 헤드리스와 GUI 초기화 실패를 구별할 수 없다.
//! 사용자 설정과 셸 스크립트는 이 DB에 저장하지 않는다.

#[cfg(any(feature = "gui", test))]
mod migrations;
#[cfg(any(feature = "gui", test))]
pub(crate) mod writer;

#[cfg(any(feature = "gui", test))]
use std::io;
#[cfg(any(feature = "gui", test))]
use std::path::{Path, PathBuf};

#[cfg(any(feature = "gui", test))]
use rusqlite::Connection;

#[cfg(any(feature = "gui", test))]
pub use migrations::DbSchemaError;

#[cfg(any(feature = "gui", test))]
pub struct Db {
    pub conn: Connection,
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

/// GUI 부팅이 writer를 시작한 뒤 한 번 공개하는 공유 핸들.
#[cfg(feature = "gui")]
static SHARED: std::sync::OnceLock<writer::SharedState> = std::sync::OnceLock::new();

#[cfg(feature = "gui")]
pub fn default_db_path() -> Option<PathBuf> {
    tasty_utils::path::tasty_home().map(|d| d.join("state.db"))
}

/// 이번 실행에서 초기화를 이미 시도했는지. 실패한 뒤 창마다 다시 열어 안내가 쌓이지 않게 한다.
#[cfg(feature = "gui")]
static INIT_ATTEMPTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 실행당 한 번의 초기화 결과.
#[cfg(any(feature = "gui", test))]
pub(crate) enum InitOutcome {
    Opened(Db),
    /// 처음 실패했다. 안내는 호출자가 맡는다.
    Failed(DbInitError),
    /// 이미 시도했다. 다시 열지 않는다.
    Repeated,
}

/// GUI 부팅에서 실행당 한 번 연다. 실패한 뒤에는 같은 실행에서 다시 열지 않고, 소비자는 공유
/// 핸들이 없는 기본값을 쓴다. 원본 파일은 열기만 시도하며 지우거나 새로 만들어 덮지 않는다.
#[cfg(feature = "gui")]
pub(crate) fn init() -> InitOutcome {
    init_once(&INIT_ATTEMPTED, || {
        let path = default_db_path().ok_or(DbInitError::HomeDirMissing)?;
        Db::open(&path).map(|db| (path, db))
    })
}

#[cfg(any(feature = "gui", test))]
fn init_once(
    attempted: &std::sync::atomic::AtomicBool,
    open: impl FnOnce() -> Result<(PathBuf, Db), DbInitError>,
) -> InitOutcome {
    use std::sync::atomic::Ordering;
    if attempted.swap(true, Ordering::AcqRel) {
        return InitOutcome::Repeated;
    }
    match open() {
        Ok((path, db)) => {
            tracing::info!("opened state.db at {}", path.display());
            InitOutcome::Opened(db)
        }
        Err(error) => {
            tracing::warn!(
                "state.db init failed: {error} — continuing without recent files and tutorial progress"
            );
            InitOutcome::Failed(error)
        }
    }
}

/// writer를 시작한 부팅이 공유 핸들을 공개한다. 실행당 한 번이다.
#[cfg(feature = "gui")]
pub(crate) fn publish(shared: writer::SharedState) {
    if SHARED.set(shared).is_err() {
        tracing::error!("state.db shared handle was published twice; keeping the first");
    }
}

/// 모든 창이 공유하는 최근 파일 캐시. state.db writer가 없으면 None이다.
pub(crate) fn shared_recent_files() -> Option<crate::recent_files::RecentFiles> {
    #[cfg(feature = "gui")]
    {
        SHARED.get().map(|s| s.recent.clone())
    }
    #[cfg(not(feature = "gui"))]
    {
        None
    }
}

/// 튜토리얼 저장을 넣을 writer 큐. state.db writer가 없으면 None이다.
#[cfg(feature = "gui")]
pub(crate) fn shared_queue() -> Option<writer::StateQueue> {
    SHARED.get().map(|s| s.queue.clone())
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
        let open = || Db::open(&path).map(|db| (path.clone(), db));
        let first = init_once(&attempted, open);
        assert!(
            matches!(first, InitOutcome::Failed(DbInitError::Corrupt(_))),
            "a corrupt file must be reported"
        );
        let mut calls = 0;
        let second = init_once(&attempted, || {
            calls += 1;
            Db::open(&path).map(|db| (path.clone(), db))
        });
        assert!(matches!(second, InitOutcome::Repeated));
        assert_eq!(calls, 0, "the same run must not reopen after a failure");
        assert_eq!(std::fs::read(&path).unwrap(), junk);
    }

    /// 처음 연 연결을 넘겨주고, 같은 실행의 다음 시도는 다시 열지 않는다.
    #[test]
    fn a_successful_open_hands_over_the_connection_once() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("state.db");
        let attempted = std::sync::atomic::AtomicBool::new(false);
        let opened = init_once(&attempted, || Db::open(&path).map(|db| (path.clone(), db)));
        let InitOutcome::Opened(db) = opened else {
            panic!("the first open must hand over the connection");
        };
        let version: u32 = db
            .conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(version, migrations::SCHEMA_VERSION);
        assert!(matches!(
            init_once(&attempted, || unreachable!("must not reopen")),
            InitOutcome::Repeated
        ));
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
