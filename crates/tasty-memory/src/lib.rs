//! 에이전트 메모리 저장소 (`~/.tasty/memory.db`).
//!
//! 에이전트가 작업 도중 누적·검색·공유하는 영속 키-값. 스코프(`global` /
//! `account:<u>` / `window:<id>` / `workspace:<id>` / `surface:<id>`)로 가시성
//! 제어. SQLite WAL 모드 단일 파일.
//!
//! ## 동기 모델
//!
//! Tasty 본 바이너리는 winit 이벤트 루프 + sync 코드 베이스다 (tokio 사용 안 함).
//! 따라서 `MemoryStore`는 `OnceLock<Mutex<Connection>>` 싱글톤 + `with_store(...)`
//! 동기 콜백 패턴. IPC dispatch는 메인 스레드에서 순차 호출되고, plugin process
//! 호출도 별도 스레드의 mpsc 경로를 거쳐 결국 메인에서 처리되므로 단일 mutex로
//! 충분 (1 MiB 캡 + WAL 모드라 lock holding time도 짧다).
//!
//! ## 키 / 스코프 규칙
//!
//! - 키: 1..=256자, `[a-z0-9._-]+`. 점 표기로 계층(`task.123.plan`).
//! - 예약 prefix: `tasty.` (호스트 내부), `plugin.<plugin-id>.` (각 plugin namespace).
//! - 값: bytes + content_type. `application/json` / `text/plain` /
//!   `application/octet-stream`. 단일 값 ≤ 1 MiB.

mod migrations;
mod scope;

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, ErrorCode, OptionalExtension, params};
use serde::{Deserialize, Serialize};

pub use migrations::{DbSchemaError, SCHEMA_VERSION};
pub use scope::{Scope, validate_key};

/// 단일 값 최대 크기 (1 MiB). 초과 시 `ValueTooLarge`.
pub const MAX_VALUE_BYTES: usize = 1024 * 1024;

/// `MemoryStore`/CRUD 공용 에러.
#[derive(Debug, thiserror::Error)]
pub enum MemoryError {
    #[error("memory entry not found: {scope} / {key}")]
    NotFound { scope: String, key: String },
    #[error("CAS conflict: expected v{expected}, got v{actual}")]
    CasConflict { expected: u64, actual: u64 },
    #[error("invalid key: {0}")]
    InvalidKey(String),
    #[error("invalid scope: {0}")]
    InvalidScope(String),
    #[error("invalid content type: {0}")]
    InvalidContentType(String),
    #[error("value too large: {actual} bytes (max {max})")]
    ValueTooLarge { actual: usize, max: usize },
    #[error("db error: {0}")]
    Db(#[from] rusqlite::Error),
}

pub type Result<T> = std::result::Result<T, MemoryError>;

/// 값 페이로드. `text`/`json`은 UTF-8, `binary`는 임의 바이트.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MemoryValue {
    /// `text/plain` — UTF-8 문자열. surface.meta 호환 표면이 사용하는 기본 표현.
    Text(String),
    /// `application/json` — 임의 JSON 값. caller가 미리 직렬화한 문자열을 보존.
    Json(serde_json::Value),
    /// `application/octet-stream` — 임의 바이트열.
    Binary(Vec<u8>),
}

impl MemoryValue {
    fn content_type(&self) -> &'static str {
        match self {
            MemoryValue::Text(_) => "text/plain",
            MemoryValue::Json(_) => "application/json",
            MemoryValue::Binary(_) => "application/octet-stream",
        }
    }

    fn to_bytes(&self) -> std::result::Result<Vec<u8>, serde_json::Error> {
        Ok(match self {
            MemoryValue::Text(s) => s.as_bytes().to_vec(),
            MemoryValue::Json(v) => serde_json::to_vec(v)?,
            MemoryValue::Binary(b) => b.clone(),
        })
    }

    fn from_db(content_type: &str, bytes: Vec<u8>) -> Result<Self> {
        match content_type {
            "text/plain" => {
                let s = String::from_utf8(bytes)
                    .map_err(|e| MemoryError::InvalidContentType(format!("text/plain not utf8: {e}")))?;
                Ok(MemoryValue::Text(s))
            }
            "application/json" => {
                let v: serde_json::Value = serde_json::from_slice(&bytes)
                    .map_err(|e| MemoryError::InvalidContentType(format!("invalid json: {e}")))?;
                Ok(MemoryValue::Json(v))
            }
            "application/octet-stream" => Ok(MemoryValue::Binary(bytes)),
            other => Err(MemoryError::InvalidContentType(other.to_string())),
        }
    }
}

/// 한 entry. `version`은 다음 CAS update에서 expected로 넘길 값.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub scope: String,
    pub key: String,
    pub value: MemoryValue,
    pub created_at: i64,
    pub updated_at: i64,
    pub expires_at: Option<i64>,
    pub version: u64,
}

/// `put` 옵션.
#[derive(Debug, Default, Clone)]
pub struct PutOpts {
    /// 절대 만료 시각 (unix ms). `None`이면 영구.
    pub expires_at: Option<i64>,
    /// 낙관 락. 일치하지 않으면 `CasConflict`.
    pub cas: Option<u64>,
}

/// `list` 옵션.
#[derive(Debug, Default, Clone)]
pub struct ListOpts {
    pub prefix: Option<String>,
    pub limit: Option<usize>,
}

/// MemoryStore. 디스크 파일을 단독으로 열어 mutex 보호. clone 불가.
pub struct MemoryStore {
    conn: Connection,
}

impl MemoryStore {
    /// 디스크 파일 열기. 부모 디렉터리는 필요 시 생성. 스키마는 자동 적용/검증.
    pub fn open(path: &Path) -> std::result::Result<Self, MemoryInitError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| classify_io(e, parent))?;
        }
        let conn = Connection::open(path).map_err(|e| classify_sql(e, path))?;
        Self::prepare(conn, path)
    }

    /// 인메모리 (테스트 전용).
    #[cfg(test)]
    pub fn open_in_memory() -> std::result::Result<Self, MemoryInitError> {
        let conn = Connection::open_in_memory()
            .map_err(|e| classify_sql(e, Path::new(":memory:")))?;
        Self::prepare(conn, Path::new(":memory:"))
    }

    fn prepare(mut conn: Connection, path: &Path) -> std::result::Result<Self, MemoryInitError> {
        conn.pragma_update(None, "journal_mode", "WAL").ok();
        conn.pragma_update(None, "synchronous", "NORMAL").ok();
        conn.pragma_update(None, "foreign_keys", "ON").ok();
        migrations::ensure_schema(&mut conn).map_err(|e| match e {
            DbSchemaError::SchemaMismatch { expected, found } => {
                MemoryInitError::SchemaMismatch { expected, found }
            }
            DbSchemaError::Sql(e) => classify_sql(e, path),
        })?;
        Ok(Self { conn })
    }

    /// 값을 저장(upsert). 신규면 version=1, 갱신이면 +1. CAS 미스면 conflict.
    /// 반환: 새 version.
    pub fn put(
        &mut self,
        scope: &Scope,
        key: &str,
        value: &MemoryValue,
        opts: &PutOpts,
    ) -> Result<u64> {
        validate_key(key).map_err(MemoryError::InvalidKey)?;
        let bytes = value
            .to_bytes()
            .map_err(|e| MemoryError::InvalidContentType(e.to_string()))?;
        if bytes.len() > MAX_VALUE_BYTES {
            return Err(MemoryError::ValueTooLarge {
                actual: bytes.len(),
                max: MAX_VALUE_BYTES,
            });
        }
        let scope_token = scope.as_token();
        let now = unix_ms_now();
        let content_type = value.content_type();

        let tx = self.conn.transaction()?;

        let existing: Option<u64> = tx
            .query_row(
                "SELECT version FROM memory WHERE scope=?1 AND key=?2",
                params![&scope_token, key],
                |r| r.get::<_, i64>(0).map(|v| v as u64),
            )
            .optional()?;

        let new_version = match (existing, opts.cas) {
            (None, Some(expected)) => {
                return Err(MemoryError::CasConflict {
                    expected,
                    actual: 0,
                });
            }
            (Some(actual), Some(expected)) if actual != expected => {
                return Err(MemoryError::CasConflict { expected, actual });
            }
            (Some(actual), _) => {
                let new_v = actual + 1;
                tx.execute(
                    "UPDATE memory SET value=?1, content_type=?2, updated_at=?3, expires_at=?4, version=?5
                     WHERE scope=?6 AND key=?7",
                    params![
                        &bytes,
                        content_type,
                        now,
                        opts.expires_at,
                        new_v as i64,
                        &scope_token,
                        key
                    ],
                )?;
                new_v
            }
            (None, _) => {
                tx.execute(
                    "INSERT INTO memory (scope, key, value, content_type, created_at, updated_at, expires_at, version)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?6, 1)",
                    params![&scope_token, key, &bytes, content_type, now, opts.expires_at],
                )?;
                1
            }
        };

        tx.commit()?;
        Ok(new_version)
    }

    /// 단건 조회. 만료된 키는 `None` 취급.
    pub fn get(&self, scope: &Scope, key: &str) -> Result<Option<MemoryEntry>> {
        validate_key(key).map_err(MemoryError::InvalidKey)?;
        let scope_token = scope.as_token();
        let now = unix_ms_now();

        let row = self.conn
            .query_row(
                "SELECT value, content_type, created_at, updated_at, expires_at, version
                 FROM memory WHERE scope=?1 AND key=?2",
                params![&scope_token, key],
                |r| {
                    Ok((
                        r.get::<_, Vec<u8>>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, i64>(2)?,
                        r.get::<_, i64>(3)?,
                        r.get::<_, Option<i64>>(4)?,
                        r.get::<_, i64>(5)? as u64,
                    ))
                },
            )
            .optional()?;

        let Some((bytes, ct, created_at, updated_at, expires_at, version)) = row else {
            return Ok(None);
        };
        if let Some(exp) = expires_at
            && exp <= now
        {
            return Ok(None);
        }
        let value = MemoryValue::from_db(&ct, bytes)?;
        Ok(Some(MemoryEntry {
            scope: scope_token,
            key: key.to_string(),
            value,
            created_at,
            updated_at,
            expires_at,
            version,
        }))
    }

    /// 키 존재 여부. 만료된 키는 false.
    pub fn exists(&self, scope: &Scope, key: &str) -> Result<bool> {
        Ok(self.get(scope, key)?.is_some())
    }

    /// 삭제. CAS 미스면 conflict. 없으면 NotFound.
    pub fn delete(&mut self, scope: &Scope, key: &str, cas: Option<u64>) -> Result<()> {
        validate_key(key).map_err(MemoryError::InvalidKey)?;
        let scope_token = scope.as_token();

        let tx = self.conn.transaction()?;
        let existing: Option<u64> = tx
            .query_row(
                "SELECT version FROM memory WHERE scope=?1 AND key=?2",
                params![&scope_token, key],
                |r| r.get::<_, i64>(0).map(|v| v as u64),
            )
            .optional()?;

        match (existing, cas) {
            (None, _) => {
                return Err(MemoryError::NotFound {
                    scope: scope_token,
                    key: key.to_string(),
                });
            }
            (Some(actual), Some(expected)) if actual != expected => {
                return Err(MemoryError::CasConflict { expected, actual });
            }
            _ => {}
        }

        tx.execute(
            "DELETE FROM memory WHERE scope=?1 AND key=?2",
            params![&scope_token, key],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// 스코프 내 키 리스트. 만료된 키는 제외. prefix 옵션, limit 옵션.
    pub fn list(&self, scope: &Scope, opts: &ListOpts) -> Result<Vec<MemoryEntry>> {
        let scope_token = scope.as_token();
        let now = unix_ms_now();
        let limit = opts.limit.unwrap_or(usize::MAX) as i64;

        let mut sql = String::from(
            "SELECT key, value, content_type, created_at, updated_at, expires_at, version
             FROM memory
             WHERE scope=?1 AND (expires_at IS NULL OR expires_at > ?2)",
        );
        if opts.prefix.is_some() {
            sql.push_str(" AND key LIKE ?3 ESCAPE '\\'");
        }
        sql.push_str(" ORDER BY key ASC LIMIT ?");
        sql.push_str(if opts.prefix.is_some() { "4" } else { "3" });

        let mut stmt = self.conn.prepare(&sql)?;
        let mut entries = Vec::new();
        let map_row = |r: &rusqlite::Row<'_>| -> rusqlite::Result<(
            String,
            Vec<u8>,
            String,
            i64,
            i64,
            Option<i64>,
            u64,
        )> {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get::<_, i64>(6)? as u64,
            ))
        };

        if let Some(prefix) = &opts.prefix {
            let like = format!("{}%", escape_like(prefix));
            let rows = stmt.query_map(params![&scope_token, now, like, limit], map_row)?;
            for row in rows {
                let (key, bytes, ct, ca, ua, ea, ver) = row?;
                entries.push(MemoryEntry {
                    scope: scope_token.clone(),
                    key,
                    value: MemoryValue::from_db(&ct, bytes)?,
                    created_at: ca,
                    updated_at: ua,
                    expires_at: ea,
                    version: ver,
                });
            }
        } else {
            let rows = stmt.query_map(params![&scope_token, now, limit], map_row)?;
            for row in rows {
                let (key, bytes, ct, ca, ua, ea, ver) = row?;
                entries.push(MemoryEntry {
                    scope: scope_token.clone(),
                    key,
                    value: MemoryValue::from_db(&ct, bytes)?,
                    created_at: ca,
                    updated_at: ua,
                    expires_at: ea,
                    version: ver,
                });
            }
        }
        Ok(entries)
    }

    /// 스코프 내 키 갯수 (만료 제외, prefix 옵션).
    pub fn count(&self, scope: &Scope, prefix: Option<&str>) -> Result<u64> {
        let scope_token = scope.as_token();
        let now = unix_ms_now();
        let n: i64 = if let Some(p) = prefix {
            let like = format!("{}%", escape_like(p));
            self.conn.query_row(
                "SELECT COUNT(*) FROM memory
                 WHERE scope=?1 AND (expires_at IS NULL OR expires_at > ?2)
                   AND key LIKE ?3 ESCAPE '\\'",
                params![&scope_token, now, like],
                |r| r.get(0),
            )?
        } else {
            self.conn.query_row(
                "SELECT COUNT(*) FROM memory
                 WHERE scope=?1 AND (expires_at IS NULL OR expires_at > ?2)",
                params![&scope_token, now],
                |r| r.get(0),
            )?
        };
        Ok(n as u64)
    }

    /// 사용 중인 스코프 목록.
    pub fn scopes(&self) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT scope FROM memory ORDER BY scope ASC",
        )?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// 통계. 스코프별 (또는 전체) entry count + 총 byte size.
    pub fn stats(&self, scope: Option<&Scope>) -> Result<MemoryStats> {
        let now = unix_ms_now();
        if let Some(s) = scope {
            let token = s.as_token();
            let (count, bytes): (i64, i64) = self.conn.query_row(
                "SELECT COUNT(*), COALESCE(SUM(LENGTH(value)), 0) FROM memory
                 WHERE scope=?1 AND (expires_at IS NULL OR expires_at > ?2)",
                params![&token, now],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            Ok(MemoryStats {
                scope: Some(token),
                entries: count as u64,
                bytes: bytes as u64,
            })
        } else {
            let (count, bytes): (i64, i64) = self.conn.query_row(
                "SELECT COUNT(*), COALESCE(SUM(LENGTH(value)), 0) FROM memory
                 WHERE (expires_at IS NULL OR expires_at > ?1)",
                params![now],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            Ok(MemoryStats {
                scope: None,
                entries: count as u64,
                bytes: bytes as u64,
            })
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryStats {
    pub scope: Option<String>,
    pub entries: u64,
    pub bytes: u64,
}

/// Init/open 실패. `MemoryError`와 분리 — 호출자가 사용자 친화 메시지를 띄울
/// 때 분기가 필요해서.
#[derive(Debug)]
pub enum MemoryInitError {
    HomeDirMissing,
    PermissionDenied(PathBuf),
    Busy(PathBuf),
    DiskFull,
    Corrupt(PathBuf),
    SchemaMismatch { expected: u32, found: u32 },
    Other(String),
}

impl MemoryInitError {
    pub fn user_message_i18n(&self) -> (&'static str, Vec<String>) {
        match self {
            MemoryInitError::HomeDirMissing => ("memory_error.home_missing", vec![]),
            MemoryInitError::PermissionDenied(p) => (
                "memory_error.permission_denied",
                vec![p.display().to_string()],
            ),
            MemoryInitError::Busy(p) => ("memory_error.busy", vec![p.display().to_string()]),
            MemoryInitError::DiskFull => ("memory_error.disk_full", vec![]),
            MemoryInitError::Corrupt(p) => ("memory_error.corrupt", vec![p.display().to_string()]),
            MemoryInitError::SchemaMismatch { expected, found } => (
                "memory_error.schema_mismatch",
                vec![expected.to_string(), found.to_string()],
            ),
            MemoryInitError::Other(msg) => ("memory_error.other", vec![msg.clone()]),
        }
    }
}

impl std::fmt::Display for MemoryInitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MemoryInitError::HomeDirMissing => write!(f, "home directory missing"),
            MemoryInitError::PermissionDenied(p) => write!(f, "permission denied: {}", p.display()),
            MemoryInitError::Busy(p) => write!(f, "memory.db busy: {}", p.display()),
            MemoryInitError::DiskFull => write!(f, "disk full"),
            MemoryInitError::Corrupt(p) => write!(f, "memory.db corrupted: {}", p.display()),
            MemoryInitError::SchemaMismatch { expected, found } => {
                write!(f, "memory.db schema mismatch (expected {expected}, found {found})")
            }
            MemoryInitError::Other(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for MemoryInitError {}

fn classify_io(err: std::io::Error, path: &Path) -> MemoryInitError {
    match err.kind() {
        std::io::ErrorKind::PermissionDenied => MemoryInitError::PermissionDenied(path.to_path_buf()),
        _ if err.raw_os_error() == Some(enospc()) => MemoryInitError::DiskFull,
        _ => MemoryInitError::Other(format!("{}: {err}", path.display())),
    }
}

#[cfg(unix)]
fn enospc() -> i32 {
    28
}
#[cfg(windows)]
fn enospc() -> i32 {
    112
}

fn classify_sql(err: rusqlite::Error, path: &Path) -> MemoryInitError {
    if let rusqlite::Error::SqliteFailure(sqlite_err, _) = &err {
        match sqlite_err.code {
            ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked => {
                return MemoryInitError::Busy(path.to_path_buf());
            }
            ErrorCode::DatabaseCorrupt | ErrorCode::NotADatabase => {
                return MemoryInitError::Corrupt(path.to_path_buf());
            }
            ErrorCode::DiskFull => return MemoryInitError::DiskFull,
            ErrorCode::PermissionDenied | ErrorCode::CannotOpen => {
                return MemoryInitError::PermissionDenied(path.to_path_buf());
            }
            _ => {}
        }
    }
    MemoryInitError::Other(format!("{}: {err}", path.display()))
}

/// `~/.tasty/memory.db` 기본 경로. 홈 디렉터리 미확인 시 `None`.
pub fn default_db_path() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|dirs| dirs.home_dir().join(".tasty").join("memory.db"))
}

// ---- 싱글톤 ----

static STORE: OnceLock<Mutex<MemoryStore>> = OnceLock::new();

/// 앱 시작 시 1회. 실패 시 호출자가 사용자에게 안내 후 종료.
pub fn init() -> std::result::Result<(), MemoryInitError> {
    if STORE.get().is_some() {
        return Ok(());
    }
    let path = default_db_path().ok_or(MemoryInitError::HomeDirMissing)?;
    let store = MemoryStore::open(&path)?;
    tracing::info!("opened memory.db at {}", path.display());
    let _ = STORE.set(Mutex::new(store));
    Ok(())
}

/// 테스트: 이미 열린 store를 싱글톤으로 등록.
#[cfg(test)]
pub fn init_with(store: MemoryStore) {
    let _ = STORE.set(Mutex::new(store));
}

/// 싱글톤 접근. `init()` 전이면 `None`.
pub fn with_store<T>(f: impl FnOnce(&mut MemoryStore) -> T) -> Option<T> {
    let mutex = STORE.get()?;
    let mut guard: MutexGuard<'_, MemoryStore> = mutex.lock().ok()?;
    Some(f(&mut guard))
}

fn unix_ms_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn escape_like(input: &str) -> String {
    // SQLite LIKE 와일드카드: `%` `_`. 백슬래시로 escape (`ESCAPE '\\'` 지시).
    let mut out = String::with_capacity(input.len());
    for c in input.chars() {
        match c {
            '%' | '_' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> MemoryStore {
        MemoryStore::open_in_memory().unwrap()
    }

    fn text(s: &str) -> MemoryValue {
        MemoryValue::Text(s.into())
    }

    // M1: 기본 CRUD.
    #[test]
    fn put_get_delete_roundtrip() {
        let mut s = store();
        let scope = Scope::Surface(1);

        let v1 = s.put(&scope, "a", &text("hello"), &PutOpts::default()).unwrap();
        assert_eq!(v1, 1);

        let entry = s.get(&scope, "a").unwrap().unwrap();
        assert_eq!(entry.value, text("hello"));
        assert_eq!(entry.version, 1);
        assert!(entry.created_at <= entry.updated_at);

        let v2 = s.put(&scope, "a", &text("world"), &PutOpts::default()).unwrap();
        assert_eq!(v2, 2);

        let entry = s.get(&scope, "a").unwrap().unwrap();
        assert_eq!(entry.value, text("world"));
        assert_eq!(entry.version, 2);

        s.delete(&scope, "a", None).unwrap();
        assert!(s.get(&scope, "a").unwrap().is_none());
        assert!(!s.exists(&scope, "a").unwrap());
    }

    // M2: 스코프 격리.
    #[test]
    fn scopes_are_isolated() {
        let mut s = store();
        s.put(&Scope::Surface(1), "k", &text("s1"), &PutOpts::default()).unwrap();
        s.put(&Scope::Surface(2), "k", &text("s2"), &PutOpts::default()).unwrap();
        s.put(&Scope::Global, "k", &text("g"), &PutOpts::default()).unwrap();

        assert_eq!(s.get(&Scope::Surface(1), "k").unwrap().unwrap().value, text("s1"));
        assert_eq!(s.get(&Scope::Surface(2), "k").unwrap().unwrap().value, text("s2"));
        assert_eq!(s.get(&Scope::Global, "k").unwrap().unwrap().value, text("g"));

        // 한 스코프 delete가 다른 스코프에 영향 없음.
        s.delete(&Scope::Surface(1), "k", None).unwrap();
        assert!(s.get(&Scope::Surface(1), "k").unwrap().is_none());
        assert!(s.get(&Scope::Surface(2), "k").unwrap().is_some());
        assert!(s.get(&Scope::Global, "k").unwrap().is_some());
    }

    // M7: CAS conflict.
    #[test]
    fn cas_conflict_blocks_update() {
        let mut s = store();
        let scope = Scope::Workspace(1);
        s.put(&scope, "k", &text("v1"), &PutOpts::default()).unwrap();

        let err = s.put(
            &scope,
            "k",
            &text("v2"),
            &PutOpts { cas: Some(99), ..Default::default() },
        ).unwrap_err();
        assert!(matches!(err, MemoryError::CasConflict { actual: 1, expected: 99 }));

        // 올바른 cas는 성공.
        s.put(
            &scope,
            "k",
            &text("v2"),
            &PutOpts { cas: Some(1), ..Default::default() },
        ).unwrap();

        // 신규 키에 cas 주면 expected=N, actual=0.
        let err = s.put(
            &scope,
            "new",
            &text("x"),
            &PutOpts { cas: Some(1), ..Default::default() },
        ).unwrap_err();
        assert!(matches!(err, MemoryError::CasConflict { expected: 1, actual: 0 }));
    }

    // M4: TTL.
    #[test]
    fn expired_keys_treated_as_missing() {
        let mut s = store();
        let scope = Scope::Surface(1);
        // unix_ms_now()는 millis 단위. -1초 → 즉시 만료.
        let past = unix_ms_now() - 1000;
        s.put(
            &scope,
            "k",
            &text("v"),
            &PutOpts { expires_at: Some(past), ..Default::default() },
        ).unwrap();
        assert!(s.get(&scope, "k").unwrap().is_none());
        assert!(!s.exists(&scope, "k").unwrap());
        assert_eq!(s.count(&scope, None).unwrap(), 0);
        assert!(s.list(&scope, &ListOpts::default()).unwrap().is_empty());
    }

    // M10: 크기 제한.
    #[test]
    fn value_size_cap_enforced() {
        let mut s = store();
        let big = vec![0u8; MAX_VALUE_BYTES + 1];
        let err = s.put(
            &Scope::Global,
            "k",
            &MemoryValue::Binary(big),
            &PutOpts::default(),
        ).unwrap_err();
        assert!(matches!(err, MemoryError::ValueTooLarge { .. }));
    }

    #[test]
    fn invalid_key_rejected() {
        let mut s = store();
        let err = s.put(&Scope::Global, "BAD", &text("x"), &PutOpts::default()).unwrap_err();
        assert!(matches!(err, MemoryError::InvalidKey(_)));
    }

    #[test]
    fn delete_missing_returns_not_found() {
        let mut s = store();
        let err = s.delete(&Scope::Global, "ghost", None).unwrap_err();
        assert!(matches!(err, MemoryError::NotFound { .. }));
    }

    #[test]
    fn list_prefix_and_limit() {
        let mut s = store();
        let scope = Scope::Surface(1);
        for k in ["a.1", "a.2", "b.1", "b.2", "c.1"] {
            s.put(&scope, k, &text(k), &PutOpts::default()).unwrap();
        }

        let all = s.list(&scope, &ListOpts::default()).unwrap();
        assert_eq!(all.len(), 5);

        let a_only = s.list(&scope, &ListOpts { prefix: Some("a.".into()), limit: None }).unwrap();
        assert_eq!(a_only.iter().map(|e| e.key.as_str()).collect::<Vec<_>>(), vec!["a.1", "a.2"]);

        let limited = s.list(&scope, &ListOpts { prefix: None, limit: Some(2) }).unwrap();
        assert_eq!(limited.len(), 2);

        assert_eq!(s.count(&scope, None).unwrap(), 5);
        assert_eq!(s.count(&scope, Some("b.")).unwrap(), 2);
    }

    #[test]
    fn list_prefix_escapes_wildcards() {
        let mut s = store();
        let scope = Scope::Surface(1);
        // 키에 와일드카드 문자 자체가 못 들어가지만, prefix에 들어오는 _ 는 escape돼야 한다.
        // (key 검증 통과 키만 들어가니 실제로는 안전하나, escape_like가 호출되는지 회귀 검증.)
        s.put(&scope, "a_b", &text("v"), &PutOpts::default()).unwrap();
        s.put(&scope, "axb", &text("v"), &PutOpts::default()).unwrap();
        let only_underscore = s
            .list(&scope, &ListOpts { prefix: Some("a_".into()), limit: None })
            .unwrap();
        assert_eq!(only_underscore.iter().map(|e| e.key.as_str()).collect::<Vec<_>>(), vec!["a_b"]);
    }

    #[test]
    fn json_value_roundtrip() {
        let mut s = store();
        let json = MemoryValue::Json(serde_json::json!({ "n": 1, "xs": [true, "a"] }));
        s.put(&Scope::Global, "k", &json, &PutOpts::default()).unwrap();
        assert_eq!(s.get(&Scope::Global, "k").unwrap().unwrap().value, json);
    }

    #[test]
    fn binary_value_roundtrip() {
        let mut s = store();
        let bin = MemoryValue::Binary(vec![0, 1, 2, 255, 7]);
        s.put(&Scope::Global, "k", &bin, &PutOpts::default()).unwrap();
        assert_eq!(s.get(&Scope::Global, "k").unwrap().unwrap().value, bin);
    }

    #[test]
    fn scopes_listing_and_stats() {
        let mut s = store();
        s.put(&Scope::Global, "a", &text("x"), &PutOpts::default()).unwrap();
        s.put(&Scope::Surface(1), "a", &text("y"), &PutOpts::default()).unwrap();
        s.put(&Scope::Surface(2), "a", &text("z"), &PutOpts::default()).unwrap();

        let scopes = s.scopes().unwrap();
        assert_eq!(scopes, vec!["global", "surface:1", "surface:2"]);

        let global_stats = s.stats(Some(&Scope::Global)).unwrap();
        assert_eq!(global_stats.entries, 1);
        assert_eq!(global_stats.bytes, 1);

        let total = s.stats(None).unwrap();
        assert_eq!(total.entries, 3);
        assert!(total.bytes >= 3);
    }
}
