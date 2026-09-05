// 이유: 이 저장소를 여닫는 것이 gui 부팅 경로뿐이라 headless 빌드엔 호출자가 없다. 모듈을
// `#[cfg]` 로 가리지 않는 것은 headless 에서도 타입체크를 받게 하려는 것이다.
#![cfg_attr(not(feature = "gui"), allow(dead_code, unused_imports))]
//! SQLite 기반 영속 상태 저장소 (`~/.tasty/state.db`).
//!
//! 대상 도메인:
//! - 최근 파일 (markdown / html)
//! - 클립보드 히스토리 스키마 자리 (실제 기록 연결은 별도 단계)
//!
//! 사용자 설정(config.toml)이나 쉘 스크립트(bashrc)는 이 저장소에
//! 들어가지 않는다 — 텍스트 편집/버전관리 대상은 그대로 파일 유지.
//!
//! 접근 규칙:
//! - 메인 프로세스 단독 접근. 자식 CLI 프로세스는 IPC로 메인에 위임한다.
//! - 전역 `static` 싱글톤을 통해 어떤 코드라도 `with_db(|db| ...)`로 접근 가능.
//! - `init()`이 먼저 호출되어야 함. 실패하면 `DbInitError`로 반환되며,
//!   호출자는 사용자에게 안내한 뒤 종료해야 한다 — 인메모리 폴백 없음.

mod migrations;

use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};

use rusqlite::{Connection, ErrorCode};

pub use migrations::DbSchemaError;

pub struct Db {
    pub conn: Connection,
}

impl Db {
    /// 디스크 경로로 엶. 실패 시 Err.
    pub fn open(path: &Path) -> Result<Self, DbInitError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| classify_io(e, parent))?;
        }
        let conn = Connection::open(path).map_err(|e| classify_sql(e, path))?;
        Self::prepare(conn, path)
    }

    fn prepare(mut conn: Connection, path: &Path) -> Result<Self, DbInitError> {
        // WAL: 동시 read/write 부담 완화. synchronous=NORMAL: WAL과 궁합 좋음.
        // foreign_keys: PK 제약 정확성.
        conn.pragma_update(None, "journal_mode", "WAL").ok();
        conn.pragma_update(None, "synchronous", "NORMAL").ok();
        conn.pragma_update(None, "foreign_keys", "ON").ok();
        // WAL 크기 상한. state.db 는 memory.db 와 **별개의 prepare** 를 쓰므로
        // (같은 세 pragma 를 각자 박아 둔 형태) 한쪽만 고치면 다른 쪽은 그대로
        // 무한히 자란다. 값의 근거는 `tasty_memory::WAL_SIZE_LIMIT_BYTES` doc 참조 —
        // 두 DB 가 같은 SQLite 기본값(page_size 4096 · wal_autocheckpoint 1000)을
        // 쓰므로 상한도 같은 값을 공유한다.
        if let Err(e) = conn.pragma_update(
            None,
            "journal_size_limit",
            tasty_memory::WAL_SIZE_LIMIT_BYTES,
        ) {
            tracing::warn!(
                "{}: failed to set journal_size_limit; the WAL file can grow without bound: {e}",
                path.display()
            );
        }

        migrations::ensure_schema(&mut conn).map_err(|e| match e {
            DbSchemaError::SchemaMismatch { expected, found } => {
                DbInitError::SchemaMismatch { expected, found }
            }
            DbSchemaError::Sql(e) => classify_sql(e, path),
        })?;
        Ok(Self { conn })
    }
}

/// `init()` 결과. 각 variant가 사용자에게 보여줄 i18n key와 인자를 알고 있다.
#[derive(Debug)]
pub enum DbInitError {
    HomeDirMissing,
    PermissionDenied(PathBuf),
    Busy(PathBuf),
    DiskFull,
    Corrupt(PathBuf),
    SchemaMismatch { expected: u32, found: u32 },
    Other(String),
}

impl DbInitError {
    /// i18n key와 포맷용 인자 0~2개. main 쪽에서 `t`/`t_fmt`/`t_fmt2`로 분기한다.
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

impl std::error::Error for DbInitError {}

fn classify_io(err: io::Error, path: &Path) -> DbInitError {
    match err.kind() {
        io::ErrorKind::PermissionDenied => DbInitError::PermissionDenied(path.to_path_buf()),
        // io::ErrorKind::StorageFull은 nightly. raw OS 코드로 우회 가능하지만
        // 실용성이 낮으므로 메시지에 의존한다.
        _ if err.raw_os_error() == Some(libc_enospc()) => DbInitError::DiskFull,
        _ => DbInitError::Other(format!("{path:?}: {err}", path = path.display())),
    }
}

#[cfg(unix)]
fn libc_enospc() -> i32 {
    28 // ENOSPC
}

#[cfg(windows)]
fn libc_enospc() -> i32 {
    112 // ERROR_DISK_FULL
}

fn classify_sql(err: rusqlite::Error, path: &Path) -> DbInitError {
    if let rusqlite::Error::SqliteFailure(sqlite_err, _) = &err {
        match sqlite_err.code {
            ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked => {
                return DbInitError::Busy(path.to_path_buf());
            }
            ErrorCode::DatabaseCorrupt | ErrorCode::NotADatabase => {
                return DbInitError::Corrupt(path.to_path_buf());
            }
            ErrorCode::DiskFull => return DbInitError::DiskFull,
            ErrorCode::PermissionDenied | ErrorCode::CannotOpen => {
                // CANTOPEN은 권한/존재/디렉터리 등 복합 원인 — 권한으로 묶는다.
                return DbInitError::PermissionDenied(path.to_path_buf());
            }
            _ => {}
        }
    }
    DbInitError::Other(format!("{}: {err}", path.display()))
}

static DB: OnceLock<Mutex<Db>> = OnceLock::new();

/// `state.db` 접근 락의 poison 을 보고했는가(첫 1 회만).
static DB_POISONED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
const DB_WHAT: &str = "state.db connection";

/// `state.db` 경로 (`tasty_home()/state.db`). `None`이면 홈 디렉터리 미확인.
pub fn default_db_path() -> Option<PathBuf> {
    tasty_utils::path::tasty_home().map(|d| d.join("state.db"))
}

/// 앱 시작 시 1회 호출. 실패하면 호출자가 사용자에게 안내하고 종료해야 한다.
pub fn init() -> Result<(), DbInitError> {
    if DB.get().is_some() {
        return Ok(());
    }
    let path = default_db_path().ok_or(DbInitError::HomeDirMissing)?;
    let db = Db::open(&path)?;
    tracing::info!("opened state.db at {}", path.display());
    // OnceLock::set은 이미 set된 경우(Err)에만 실패하며, 위의 is_some() 검사가
    // 통과해 여기 도달했으므로 race(다른 스레드가 동시 호출)인 경우만 Err.
    // 두 스레드가 동일한 default_db_path를 두고 경쟁하는 케이스라 결과는 동일하다.
    let _ = DB.set(Mutex::new(db)); // 이미 초기화된 경우 무시 (OnceLock idempotent)
    Ok(())
}

/// 싱글톤 접근. `init()`이 호출되지 않았으면 None.
///
/// poison 은 복구한다. 미완 트랜잭션은 unwind 때 rusqlite 의 RAII guard 가 rollback
/// 하므로 연결은 불변식을 유지하고, 여기서 패닉하면 메인 스레드를 포함한 아무 데서나
/// 호출되는 접근자라 창 전체가 죽는다. 조용히 `None` 을 돌려주면 호출자가 **"DB 가
/// 아직 없다" 와 "락이 깨졌다" 를 구분할 수 없어**, 설정·최근 항목 저장이 원인 없이
/// 사라진다. 근거 `docs/dev-guide/error-handling.md` "락 poison".
pub fn with_db<T>(f: impl FnOnce(&mut Db) -> T) -> Option<T> {
    let mutex = DB.get()?;
    let mut guard: MutexGuard<'_, Db> =
        crate::poison::recover_mutex(mutex.lock(), DB_WHAT, &DB_POISONED);
    Some(f(&mut guard))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `state.db` 는 `memory.db` 와 **다른 `prepare`** 를 쓴다(각 crate 가 같은
    /// pragma 를 따로 박아 둔 형태). 그래서 한쪽만 고치면 다른 쪽 WAL 은 여전히
    /// 무한히 자란다 — 두 경로가 같은 상한을 쓰는지 여기서 고정한다.
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
