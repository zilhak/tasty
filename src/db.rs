//! SQLite 기반 영속 상태 저장소 (`~/.tasty/state.db`).
//!
//! 대상 도메인:
//! - 최근 파일 (markdown / html)
//! - 클립보드 히스토리 스키마 자리 (실제 기록 연결은 별도 단계)
//!
//! 사용자 설정(config.toml)이나 쉘 스크립트(bashrc)는 이 저장소에
//! 들어가지 않는다 — 텍스트 편집/버전관리 대상은 그대로 파일 유지.
//!
//! ## 누가 이 저장소를 여는가
//!
//! **여는 쪽은 GUI 부팅 하나다.** [`init`] 과 [`default_db_path`] 는
//! `cfg(feature = "gui")` 이고, 부르는 자리는 `src/app/` 의 부팅 경로 둘뿐이다.
//! 헤드리스 빌드에는 이 DB 를 여는 코드가 **컴파일되지도 않는다** — 실측하면 헤드리스
//! 데몬의 홈에는 `memory.db` 만 생기고 `state.db` 는 파일도 로그도 남지 않는다.
//!
//! 그래서 [`with_state_db`] 가 돌려주는 `None` 은 **"열려 있지 않다"** 는 뜻이다. 락이
//! 깨진 경우는 여기 오지 않는다 — 그 함수가 복구해 `Some` 으로 돌려준다. 남는 출처는
//! **둘**이고, 이 값으로는 **둘이 안 갈린다**:
//!
//! 1. 헤드리스라 여는 코드가 아예 없다
//! 2. GUI 가 열다 실패했다
//!
//! **2 를 "앱이 끝나니까 안 보인다" 로 배제하지 마라 — 실측하면 보인다.** 부팅은
//! `init()` 의 에러를 던지지 않고 변수에 담고(`src/app/boot_machine.rs` 의
//! `init_boot_db_and_theme`), 상태를 **먼저** 만든 뒤(그 생성자 안에 소비자가 하나 있다 —
//! `AppState::new` 의 `RecentFiles::load()`) 그제서야 안내 모달을 띄운다. 그 모달의
//! `on_close` 가 `Exit(1)` 이라 **종료는 사용자가 확인을 누를 때** 나고, 그때까지 창은
//! 살아 있고 IPC 도 답한다(실측: 열기를 실패시킨 채 30 초 뒤에도 생존, `recent.query` 가
//! 헤드리스와 **글자 하나 다르지 않은** `{"recent":[]}` 를 돌려줬다).
//!
//! **소비자 계약은 그대로다**: `None` 은 오류가 아니라 "지금 이 프로세스에는 영속 저장이
//! 없다" 이고, 기본값으로 떨어지면 된다. 바뀐 것은 **그 위에 무엇을 얹으면 안 되는가**다 —
//! `None` 을 보고 *왜* 없는지 판단하지 마라. 특히 "`None` 이면 저장 실패를 사용자에게
//! 안내하지 않는다" 류의 분기를 넣으면 DB 가 깨진 사용자에게 아무 안내도 안 간다. 그
//! 구분에 필요한 `DbInitError` 는 부팅이 모달로 바꾼 뒤 **버린다**(어디에도 안 남는다).
//! 구분이 필요해지면 먼저 그 값을 남기는 것부터 해야 하고, 그것은 동작 변경이다.
//!
//! ## `memory.db` 와 섞지 마라
//!
//! 에이전트 메모리(`crates/tasty-memory/`)는 **헤드리스에서도 열린다.** 접근자 이름도
//! 다르다 — 저쪽은 `with_memory`, 이쪽은 [`with_state_db`]. 두 저장소가 공유하는 것은
//! 연결 pragma 를 거는 함수 하나뿐이고(`tasty_memory::pragma::apply_connection_pragmas`),
//! 수명·소유자·스키마 정책은 전부 별개다.
//!
//! 접근 규칙:
//! - 메인 프로세스 단독 접근. 자식 CLI 프로세스는 IPC로 메인에 위임한다.
//! - `init()`이 먼저 호출되어야 함. 실패하면 `DbInitError`로 반환되며,
//!   호출자는 사용자에게 안내한 뒤 종료해야 한다 — 인메모리 폴백 없음.

// Disk initialization belongs to GUI boot; pure database tests also exercise it.
#[cfg(any(feature = "gui", test))]
mod migrations;

#[cfg(any(feature = "gui", test))]
use std::io;
#[cfg(any(feature = "gui", test))]
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};

use rusqlite::Connection;
#[cfg(any(feature = "gui", test))]
use rusqlite::ErrorCode;

#[cfg(any(feature = "gui", test))]
pub use migrations::DbSchemaError;

pub struct Db {
    pub conn: Connection,
    /// DB 수명에 묶인 최근 파일 캐시. 창마다 새 스냅샷을 만들지 않는다.
    pub(crate) recent_files: Option<crate::recent_files::RecentFiles>,
}

#[cfg(any(feature = "gui", test))]
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
        // state.db 는 memory.db 와 **별개의 prepare** 를 쓰지만 연결 pragma 는 같아야
        // 한다. 사본을 두면 한쪽만 고쳐지므로 두 DB 가 같은 함수를 부른다 —
        // WAL·synchronous·foreign_keys 와 WAL 크기 상한, 그리고 그 결과를 어떻게
        // 관측하는지까지 그 함수의 doc 에 있다.
        tasty_memory::pragma::apply_connection_pragmas(&conn, path);

        migrations::ensure_schema(&mut conn).map_err(|e| match e {
            DbSchemaError::SchemaMismatch { expected, found } => {
                DbInitError::SchemaMismatch { expected, found }
            }
            DbSchemaError::Sql(e) => classify_sql(e, path),
        })?;
        Ok(Self {
            conn,
            recent_files: None,
        })
    }
}

/// `init()` 결과. 각 variant가 사용자에게 보여줄 i18n key와 인자를 알고 있다.
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
        // io::ErrorKind::StorageFull은 nightly. raw OS 코드로 우회 가능하지만
        // 실용성이 낮으므로 메시지에 의존한다.
        _ if err.raw_os_error() == Some(libc_enospc()) => DbInitError::DiskFull,
        _ => DbInitError::Other(format!("{path:?}: {err}", path = path.display())),
    }
}

#[cfg(unix)]
#[cfg(any(feature = "gui", test))]
fn libc_enospc() -> i32 {
    28 // ENOSPC
}

#[cfg(windows)]
#[cfg(any(feature = "gui", test))]
fn libc_enospc() -> i32 {
    112 // ERROR_DISK_FULL
}

#[cfg(any(feature = "gui", test))]
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
#[cfg(feature = "gui")]
pub fn default_db_path() -> Option<PathBuf> {
    tasty_utils::path::tasty_home().map(|d| d.join("state.db"))
}

/// 앱 시작 시 1회 호출. 실패하면 호출자가 사용자에게 안내하고 종료해야 한다.
#[cfg(feature = "gui")]
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

/// `state.db` 싱글톤 접근. 열려 있지 않으면 `None`.
///
/// **이름이 `with_db` 가 아닌 이유**: 이 크레이트에는 SQLite 접근자가 둘이고
/// (`with_memory` 가 `memory.db` 쪽), 호출부만 봐서는 어느 저장소인지 구분이 안 됐다.
/// 둘이 같은 파일에 함께 나오는 자리가 없어 오독이 조용하다.
///
/// 헤드리스 빌드의 최근 파일 조회도 이 경로를 지난다. 연결 타입과 접근자는 두 빌드가
/// 공유하되 여는 것은 GUI 부팅뿐이므로, 헤드리스에서는 항상 `None` 이다. **GUI 에서도
/// `None` 이 올 수 있다** — 열기에 실패한 창이 안내 모달을 닫기 전까지 살아 있고, 그
/// 구간을 지나는 소비자가 있다. 둘은 이 값으로 안 갈린다 — 모듈 머리말의 "누가 이
/// 저장소를 여는가" 를 봐라.
///
/// poison 은 복구한다. 미완 트랜잭션은 unwind 때 rusqlite 의 RAII guard 가 rollback
/// 하므로 연결은 불변식을 유지하고, 여기서 패닉하면 메인 스레드를 포함한 아무 데서나
/// 호출되는 접근자라 창 전체가 죽는다. 조용히 `None` 을 돌려주면 호출자가 **"DB 가
/// 아직 없다" 와 "락이 깨졌다" 를 구분할 수 없어**, 설정·최근 항목 저장이 원인 없이
/// 사라진다. 근거 `docs/dev-guide/error-handling.md` "락 poison".
pub fn with_state_db<T>(f: impl FnOnce(&mut Db) -> T) -> Option<T> {
    let mutex = DB.get()?;
    let mut guard: MutexGuard<'_, Db> =
        crate::poison::recover_mutex(mutex.lock(), DB_WHAT, &DB_POISONED);
    Some(f(&mut guard))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `state.db` 는 `memory.db` 와 **별개의 `prepare`** 를 쓰지만 연결 pragma 는 한
    /// 함수(`tasty_memory::pragma::apply_connection_pragmas`)에서 온다 — 사본이 없으니
    /// 두 DB 의 상한이 서로 갈릴 수는 없다. 갈릴 수 있는 것은 **이 경로가 그 함수를
    /// 계속 부르는가** 이고, `prepare` 에서 그 호출을 빼면 죽는 시험이 여기다.
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
