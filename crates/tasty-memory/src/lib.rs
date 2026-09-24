#![forbid(unsafe_code)]

//! SQLite 기반 키-값 저장소.
//! Regular는 읽기를 공유하고 기존 항목의 수정·삭제는 owner 또는 _host만 허용한다.
//! Secret는 owner를 기본키에 포함해 IPC에서 호출자별로 분리한다. 값은 평문 BLOB이며
//! DB 파일에 직접 접근하는 프로세스로부터 보호하지 않는다.
//!
//! owner는 호스트가 검증한 호출자로부터 정한다. 이 크레이트는 받은 문자열을 사용하므로
//! 호출자 인증이나 호스트 예약 namespace 제한은 IPC 계층의 책임이다.
//! 호스트는 MemoryStorage를 Arc<Mutex<_>>로 공유한다. 메인 IPC 처리와 워커 모두 같은
//! 저장소 락을 사용하며 MemoryStore 자체는 Sync가 아니다.

// 이유: 테스트의 let _ =를 제품 코드의 오류 처리 명부에서 제외한다.
#![cfg_attr(test, allow(clippy::let_underscore_must_use))]

mod failure;
mod fallback;
mod latency;
mod migrations;
mod port;
mod port_impl;
pub mod pragma;
mod scope;

pub mod blackboard;
pub mod cache;
pub mod goal;
pub mod plan;
pub mod testing;

pub use failure::StorageFailure;
pub use fallback::InitFallback;
pub use latency::{DbLatencySnapshot, DbLatencyStats};
pub use port::{MemoryStorage, STORE_LOCK_POISONED, STORE_LOCK_WHAT};

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

pub use migrations::{DbSchemaError, SCHEMA_VERSION};
pub use scope::{KEY_ALLOWED_CHARS, MAX_KEY_LEN, Scope, validate_key};

/// 참고용 값이며 실제 크기 제한을 집행하지 않는다.
/// 유효한 상한은 MemoryConfig::entry_max_bytes이고 호스트가 설정에서 환산한다.
pub const MAX_VALUE_BYTES: usize = 1024 * 1024;

/// Local caller (CLI / 사용자 / 호스트 내부) 의 owner sentinel. plugin id 의
/// reverse-DNS 규칙과 충돌하지 않도록 underscore prefix.
pub const HOST_OWNER: &str = "_host";

/// `MemoryStore` 의 정책 설정. 모든 cap 은 bytes 단위 (config 에서는 MiB 단위로
/// 들어와 호출자가 변환). default 는 design doc 의 fallback 값 (1 MiB / 10 MiB /
/// 1 GiB).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryConfig {
    pub entry_max_bytes: u64,
    pub secret_quota_per_owner_bytes: u64,
    pub regular_quota_total_bytes: u64,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            entry_max_bytes: 1024 * 1024,
            secret_quota_per_owner_bytes: 10 * 1024 * 1024,
            regular_quota_total_bytes: 1024 * 1024 * 1024,
        }
    }
}

/// `OwnedByOther` / `QuotaExceeded` 가 어느 영역에서 발생했는지.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MemoryArea {
    Regular,
    Secret,
}

/// `MemoryStore`/CRUD 공용 에러.
#[derive(Debug, thiserror::Error)]
pub enum MemoryError {
    #[error("memory entry not found: {scope} / {key}")]
    NotFound { scope: String, key: String },
    #[error("memory entry already exists: {scope} / {key}")]
    AlreadyExists { scope: String, key: String },
    #[error("CAS conflict: expected v{expected}, got v{actual}")]
    CasConflict { expected: u64, actual: u64 },
    #[error("entry is owned by other: {owner}")]
    OwnedByOther { owner: String },
    #[error("quota exceeded in {area:?}: used {used}, limit {limit}")]
    QuotaExceeded {
        area: MemoryArea,
        used: u64,
        limit: u64,
    },
    #[error("invalid key: {0}")]
    InvalidKey(String),
    #[error("invalid scope: {0}")]
    InvalidScope(String),
    #[error("invalid owner: {0}")]
    InvalidOwner(String),
    #[error("invalid content type: {0}")]
    InvalidContentType(String),
    #[error("value too large: {actual} bytes (max {max})")]
    ValueTooLarge { actual: usize, max: usize },
    #[error("db error: {0}")]
    Db(#[from] rusqlite::Error),
}

impl MemoryError {
    /// SQLite 저장 실패를 분류한다. NotFound·CasConflict·quota 같은 요청 거부는 None이다.
    pub fn storage_failure(&self) -> Option<StorageFailure> {
        match self {
            MemoryError::Db(e) => Some(StorageFailure::classify(e)),
            _ => None,
        }
    }
}

pub type Result<T> = std::result::Result<T, MemoryError>;

/// 값 페이로드. `text`/`json` 은 UTF-8, `binary` 는 임의 바이트.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MemoryValue {
    /// `text/plain` — UTF-8 문자열.
    Text(String),
    /// `application/json` — 임의 JSON 값.
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
                let s = String::from_utf8(bytes).map_err(|e| {
                    MemoryError::InvalidContentType(format!("text/plain not utf8: {e}"))
                })?;
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

/// 한 entry. `version` 은 다음 CAS update 에서 expected 로 넘길 값.
/// owner는 regular 응답에만 포함하고 secret 응답에서는 None이다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub scope: String,
    pub key: String,
    pub value: MemoryValue,
    pub created_at: i64,
    pub updated_at: i64,
    pub expires_at: Option<i64>,
    pub version: u64,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub owner: Option<String>,
}

/// `put` 옵션.
#[derive(Debug, Default, Clone)]
pub struct PutOpts {
    /// 절대 만료 시각 (unix ms). `None` 이면 영구.
    pub expires_at: Option<i64>,
    /// 낙관 락. 일치하지 않으면 `CasConflict`.
    pub cas: Option<u64>,
}

/// `list` 옵션.
#[derive(Debug, Default, Clone)]
pub struct ListOpts {
    pub prefix: Option<String>,
    pub limit: Option<usize>,
    /// `updated_at >= since` (unix ms). `None` 이면 하한 없음.
    pub since: Option<i64>,
    /// `updated_at < until` (unix ms). `None` 이면 상한 없음.
    pub until: Option<i64>,
    /// `key ASC` 정렬 후 건너뛸 entry 수. `None` = 0.
    pub offset: Option<usize>,
}

/// Regular 변경만 기록한다. 호스트가 take_pending_changes로 꺼내 memory.changed를 발행한다.
/// Secret 변경은 다른 플러그인에 노출하지 않는다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryChange {
    /// `surface:42` 같은 scope token (`Scope::as_token`).
    pub scope: String,
    pub key: String,
    pub kind: MemoryChangeKind,
    /// 새 version (Created/Updated 시). Deleted/Expired 는 `None`.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub version: Option<u64>,
}

/// 변경 종류.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryChangeKind {
    Created,
    Updated,
    Deleted,
    Expired,
}

/// WAL을 재사용하며 줄일 때 적용하는 journal_size_limit. 활성 WAL의 크기 상한은 아니다.
/// 자동 checkpoint 기본 페이지 수 × 기본 페이지 크기로 정하며, 실행 시험이 실제 SQLite
/// 설정과 이 관계를 대조한다. 1000 × 4096 = 4,096,000바이트로 4 MiB와 다르다.
pub const WAL_SIZE_LIMIT_BYTES: i64 = WAL_AUTOCHECKPOINT_PAGES * DEFAULT_PAGE_SIZE_BYTES;

/// SQLite 의 `wal_autocheckpoint` 기본값(페이지 수).
const WAL_AUTOCHECKPOINT_PAGES: i64 = 1000;

/// SQLite 의 `page_size` 기본값(바이트).
const DEFAULT_PAGE_SIZE_BYTES: i64 = 4096;

/// SQLite 연결을 소유한다. 공유할 때는 호출자가 mutex로 보호한다.
pub struct MemoryStore {
    conn: Connection,
    config: MemoryConfig,
    /// 호스트가 꺼내 memory.changed로 발행할 Regular 변경 목록.
    pending_changes: Vec<MemoryChange>,
    /// Regular value 바이트 합. 열 때 계산하고 put/delete에서 증감하며 purge 뒤 재계산한다.
    regular_used_bytes: i64,
    /// 저장소 mutex 없이 조회할 수 있도록 별도 Arc로 공유하는 지연 누계.
    db_latency: std::sync::Arc<DbLatencyStats>,
    /// 열 때 건 연결 pragma 의 요청값과 되읽은 실제값. 열린 뒤로 안 바뀐다.
    applied_pragmas: pragma::AppliedPragmas,
    /// `memory.db` 초기화 실패의 대체로 열렸으면 그 까닭. `fallback` 모듈 참조.
    init_fallback: Option<InitFallback>,
}

impl MemoryStore {
    /// 기본 config 로 열기. 부모 디렉터리는 필요 시 생성. 스키마는 자동 적용/검증.
    pub fn open(path: &Path) -> std::result::Result<Self, MemoryInitError> {
        Self::open_with_config(path, MemoryConfig::default())
    }

    /// 명시적 config 로 열기.
    pub fn open_with_config(
        path: &Path,
        config: MemoryConfig,
    ) -> std::result::Result<Self, MemoryInitError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| classify_io(e, parent))?;
        }
        let conn = Connection::open(path).map_err(|e| classify_sql(e, path))?;
        Self::prepare(conn, path, config)
    }

    /// 인메모리. test + production 의 placeholder 양쪽 사용.
    pub fn open_in_memory() -> std::result::Result<Self, MemoryInitError> {
        Self::open_in_memory_with_config(MemoryConfig::default())
    }

    pub fn open_in_memory_with_config(
        config: MemoryConfig,
    ) -> std::result::Result<Self, MemoryInitError> {
        let conn =
            Connection::open_in_memory().map_err(|e| classify_sql(e, Path::new(":memory:")))?;
        Self::prepare(conn, Path::new(":memory:"), config)
    }

    fn prepare(
        mut conn: Connection,
        path: &Path,
        config: MemoryConfig,
    ) -> std::result::Result<Self, MemoryInitError> {
        let applied_pragmas = crate::pragma::apply_connection_pragmas(&conn, path);
        migrations::ensure_schema(&mut conn).map_err(|e| match e {
            DbSchemaError::SchemaMismatch { expected, found } => {
                MemoryInitError::SchemaMismatch { expected, found }
            }
            DbSchemaError::Sql(e) => classify_sql(e, path),
        })?;
        let regular_used_bytes = Self::scan_regular_used(&conn);
        Ok(Self {
            conn,
            config,
            pending_changes: Vec::new(),
            regular_used_bytes,
            db_latency: std::sync::Arc::new(DbLatencyStats::default()),
            applied_pragmas,
            init_fallback: None,
        })
    }

    /// 연결 pragma의 요청값·실제값과 degraded 상태.
    pub fn applied_pragmas(&self) -> &pragma::AppliedPragmas {
        &self.applied_pragmas
    }

    /// commit·checkpoint 지연 누계 핸들. 호스트가 이것을 복제해 들고 있으면
    /// 진단 응답이 스토어 뮤텍스를 안 잡고 값을 읽는다.
    pub fn db_latency(&self) -> std::sync::Arc<DbLatencyStats> {
        self.db_latency.clone()
    }

    /// Regular 테이블의 value 바이트 총합을 전체 스캔으로 계산. open 시 1회,
    /// purge(드뭄) 후 재계산에만 쓴다 — hot path(put/delete)에서는 호출하지 않는다.
    fn scan_regular_used(conn: &Connection) -> i64 {
        conn.query_row(
            "SELECT COALESCE(SUM(LENGTH(value)), 0) FROM memory",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0)
    }

    /// 현재 적용 중인 정책.
    pub fn config(&self) -> &MemoryConfig {
        &self.config
    }

    fn check_entry_size(&self, bytes_len: usize) -> Result<()> {
        if (bytes_len as u64) > self.config.entry_max_bytes {
            return Err(MemoryError::ValueTooLarge {
                actual: bytes_len,
                max: self.config.entry_max_bytes as usize,
            });
        }
        Ok(())
    }

    // ============================================================
    // Regular memory: 공유 네임스페이스 + owner enforcement
    // ============================================================

    /// 값을 저장(upsert). 신규면 version=1 / owner=caller, 갱신이면 +1.
    /// 갱신 시 기존 owner 가 caller 와 다르면 `OwnedByOther` (caller 가 `_host` 면 root 통과).
    pub fn put(
        &mut self,
        owner: &str,
        scope: &Scope,
        key: &str,
        value: &MemoryValue,
        opts: &PutOpts,
    ) -> Result<u64> {
        validate_owner(owner)?;
        validate_key(key).map_err(MemoryError::InvalidKey)?;
        let bytes = serialize_value(value)?;
        self.check_entry_size(bytes.len())?;
        let scope_token = scope.as_token();
        let now = unix_ms_now();
        let content_type = value.content_type();
        let used_before = self.regular_used_bytes;

        let tx = self.conn.transaction()?;

        let existing: Option<(u64, String, i64)> = tx
            .query_row(
                "SELECT version, owner, LENGTH(value) FROM memory WHERE scope=?1 AND key=?2",
                params![&scope_token, key],
                |r| {
                    Ok((
                        r.get::<_, i64>(0)? as u64,
                        r.get::<_, String>(1)?,
                        r.get::<_, i64>(2)?,
                    ))
                },
            )
            .optional()?;

        // 기존 값의 크기를 빼고 새 값의 크기를 더해 전체 스캔 없이 quota를 검사한다.
        let existing_size = existing.as_ref().map(|(_, _, sz)| *sz).unwrap_or(0);
        let projected = used_before - existing_size + bytes.len() as i64;
        if (projected as u64) > self.config.regular_quota_total_bytes {
            return Err(MemoryError::QuotaExceeded {
                area: MemoryArea::Regular,
                used: projected.max(0) as u64,
                limit: self.config.regular_quota_total_bytes,
            });
        }

        let (new_version, change_kind) = match (existing, opts.cas) {
            (None, Some(expected)) => {
                return Err(MemoryError::CasConflict {
                    expected,
                    actual: 0,
                });
            }
            (Some((actual, _, _)), Some(expected)) if actual != expected => {
                return Err(MemoryError::CasConflict { expected, actual });
            }
            (Some((_, existing_owner, _)), _) if !owner_can_modify(owner, &existing_owner) => {
                return Err(MemoryError::OwnedByOther {
                    owner: existing_owner,
                });
            }
            (Some((actual, _, _)), _) => {
                let new_v = actual + 1;
                tx.execute(
                    "UPDATE memory SET value=?1, content_type=?2, updated_at=?3,
                                       expires_at=?4, version=?5
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
                (new_v, MemoryChangeKind::Updated)
            }
            (None, _) => {
                tx.execute(
                    "INSERT INTO memory
                       (scope, key, value, content_type, created_at, updated_at, expires_at,
                        version, owner)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?6, 1, ?7)",
                    params![
                        &scope_token,
                        key,
                        &bytes,
                        content_type,
                        now,
                        opts.expires_at,
                        owner
                    ],
                )?;
                (1, MemoryChangeKind::Created)
            }
        };

        let commit_started = std::time::Instant::now();
        tx.commit()?;
        self.db_latency.record_commit(commit_started.elapsed());
        // commit 성공 뒤에만 카운터를 갱신해 거절·실패한 값을 세지 않는다.
        self.regular_used_bytes = projected;
        self.pending_changes.push(MemoryChange {
            scope: scope_token,
            key: key.to_string(),
            kind: change_kind,
            version: Some(new_version),
        });
        Ok(new_version)
    }

    /// 단건 조회. 만료된 키는 `None` 취급. owner 필드 포함 (regular 의미 보존).
    pub fn get(&self, scope: &Scope, key: &str) -> Result<Option<MemoryEntry>> {
        validate_key(key).map_err(MemoryError::InvalidKey)?;
        let scope_token = scope.as_token();
        let now = unix_ms_now();

        let row = self
            .conn
            .query_row(
                "SELECT value, content_type, created_at, updated_at, expires_at, version, owner
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
                        r.get::<_, String>(6)?,
                    ))
                },
            )
            .optional()?;

        let Some((bytes, ct, created_at, updated_at, expires_at, version, owner)) = row else {
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
            owner: Some(owner),
        }))
    }

    /// 키 존재 여부. 만료된 키는 false.
    pub fn exists(&self, scope: &Scope, key: &str) -> Result<bool> {
        Ok(self.get(scope, key)?.is_some())
    }

    /// 삭제. CAS 미스면 conflict, 없으면 NotFound, owner 불일치면 OwnedByOther.
    pub fn delete(
        &mut self,
        owner: &str,
        scope: &Scope,
        key: &str,
        cas: Option<u64>,
    ) -> Result<()> {
        validate_owner(owner)?;
        validate_key(key).map_err(MemoryError::InvalidKey)?;
        let scope_token = scope.as_token();

        let tx = self.conn.transaction()?;
        let existing: Option<(u64, String, i64)> = tx
            .query_row(
                "SELECT version, owner, LENGTH(value) FROM memory WHERE scope=?1 AND key=?2",
                params![&scope_token, key],
                |r| {
                    Ok((
                        r.get::<_, i64>(0)? as u64,
                        r.get::<_, String>(1)?,
                        r.get::<_, i64>(2)?,
                    ))
                },
            )
            .optional()?;

        // 삭제될 entry 의 크기 (없으면 0). commit 후 카운터 감산에 사용.
        let deleted_size = existing.as_ref().map(|(_, _, sz)| *sz).unwrap_or(0);

        match (existing, cas) {
            (None, _) => {
                return Err(MemoryError::NotFound {
                    scope: scope_token,
                    key: key.to_string(),
                });
            }
            (Some((actual, _, _)), Some(expected)) if actual != expected => {
                return Err(MemoryError::CasConflict { expected, actual });
            }
            (Some((_, existing_owner, _)), _) if !owner_can_modify(owner, &existing_owner) => {
                return Err(MemoryError::OwnedByOther {
                    owner: existing_owner,
                });
            }
            _ => {}
        }

        tx.execute(
            "DELETE FROM memory WHERE scope=?1 AND key=?2",
            params![&scope_token, key],
        )?;
        let commit_started = std::time::Instant::now();
        tx.commit()?;
        self.db_latency.record_commit(commit_started.elapsed());
        self.regular_used_bytes = (self.regular_used_bytes - deleted_size).max(0);
        self.pending_changes.push(MemoryChange {
            scope: scope_token,
            key: key.to_string(),
            kind: MemoryChangeKind::Deleted,
            version: None,
        });
        Ok(())
    }

    /// 스코프 내 키 리스트. 만료 제외, prefix/since/until/offset/limit 옵션.
    /// 응답 entry 에 owner 포함. `key ASC` 정렬.
    pub fn list(&self, scope: &Scope, opts: &ListOpts) -> Result<Vec<MemoryEntry>> {
        let scope_token = scope.as_token();
        let now = unix_ms_now();
        let limit = opts.limit.unwrap_or(usize::MAX) as i64;
        let offset = opts.offset.unwrap_or(0) as i64;

        // 동적 WHERE/parameter 조립. 가독성 우선으로 named-position 대신 `?` 순차 사용.
        let mut sql = String::from(
            "SELECT key, value, content_type, created_at, updated_at, expires_at, version, owner
             FROM memory
             WHERE scope=? AND (expires_at IS NULL OR expires_at > ?)",
        );
        let mut binds: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        binds.push(Box::new(scope_token.clone()));
        binds.push(Box::new(now));

        if let Some(prefix) = &opts.prefix {
            sql.push_str(" AND key LIKE ? ESCAPE '\\'");
            binds.push(Box::new(format!("{}%", escape_like(prefix))));
        }
        if let Some(since) = opts.since {
            sql.push_str(" AND updated_at >= ?");
            binds.push(Box::new(since));
        }
        if let Some(until) = opts.until {
            sql.push_str(" AND updated_at < ?");
            binds.push(Box::new(until));
        }
        sql.push_str(" ORDER BY key ASC LIMIT ? OFFSET ?");
        binds.push(Box::new(limit));
        binds.push(Box::new(offset));

        let mut stmt = self.conn.prepare(&sql)?;
        let params_iter: Vec<&dyn rusqlite::ToSql> = binds.iter().map(|b| b.as_ref()).collect();
        type RegularRow = (String, Vec<u8>, String, i64, i64, Option<i64>, u64, String);
        let map_row = |r: &rusqlite::Row<'_>| -> rusqlite::Result<RegularRow> {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get::<_, i64>(6)? as u64,
                r.get(7)?,
            ))
        };

        let mut entries = Vec::new();
        let rows = stmt.query_map(params_iter.as_slice(), map_row)?;
        for row in rows {
            let (key, bytes, ct, ca, ua, ea, ver, owner) = row?;
            entries.push(MemoryEntry {
                scope: scope_token.clone(),
                key,
                value: MemoryValue::from_db(&ct, bytes)?,
                created_at: ca,
                updated_at: ua,
                expires_at: ea,
                version: ver,
                owner: Some(owner),
            });
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
        let mut stmt = self
            .conn
            .prepare("SELECT DISTINCT scope FROM memory ORDER BY scope ASC")?;
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

    /// JSON path 매칭. `key` 의 entry 가 `application/json` 일 때만 `path` 를
    /// dot 표기로 lookup 해 `expected` 와 같은 entry 만 반환한다 (Equality 비교).
    /// list 처럼 prefix/limit/offset 지원.
    ///
    /// path는 a.b.c 형식의 중첩 object 필드이며 배열 인덱스는 지원하지 않는다.
    pub fn query(
        &self,
        scope: &Scope,
        path: &str,
        expected: &serde_json::Value,
        opts: &ListOpts,
    ) -> Result<Vec<MemoryEntry>> {
        let mut list_opts = opts.clone();
        list_opts.offset = None;
        list_opts.limit = None;
        let candidates = self.list(scope, &list_opts)?;

        let mut matched: Vec<MemoryEntry> = candidates
            .into_iter()
            .filter(|e| {
                let MemoryValue::Json(v) = &e.value else {
                    return false;
                };
                lookup_dot_path(v, path)
                    .map(|hit| hit == expected)
                    .unwrap_or(false)
            })
            .collect();

        let offset = opts.offset.unwrap_or(0);
        let limit = opts.limit.unwrap_or(usize::MAX);
        if offset >= matched.len() {
            return Ok(Vec::new());
        }
        let end = (offset + limit).min(matched.len());
        Ok(matched.drain(offset..end).collect())
    }

    /// 만료되지 않은 Regular 항목을 내보낸다. scope=None이면 전체이며 Secret는 포함하지 않는다.
    pub fn export_regular(&self, scope: Option<&Scope>) -> Result<Vec<MemoryEntry>> {
        let now = unix_ms_now();
        let mut entries = Vec::new();

        let mut sql = String::from(
            "SELECT scope, key, value, content_type, created_at, updated_at, expires_at,
                    version, owner
             FROM memory
             WHERE (expires_at IS NULL OR expires_at > ?)",
        );
        let mut binds: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        binds.push(Box::new(now));
        if let Some(s) = scope {
            sql.push_str(" AND scope=?");
            binds.push(Box::new(s.as_token()));
        }
        sql.push_str(" ORDER BY scope ASC, key ASC");

        let mut stmt = self.conn.prepare(&sql)?;
        let params_iter: Vec<&dyn rusqlite::ToSql> = binds.iter().map(|b| b.as_ref()).collect();
        let rows = stmt.query_map(params_iter.as_slice(), |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Vec<u8>>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, i64>(5)?,
                r.get::<_, Option<i64>>(6)?,
                r.get::<_, i64>(7)? as u64,
                r.get::<_, String>(8)?,
            ))
        })?;
        for row in rows {
            let (scope, key, bytes, ct, ca, ua, ea, ver, owner) = row?;
            entries.push(MemoryEntry {
                scope,
                key,
                value: MemoryValue::from_db(&ct, bytes)?,
                created_at: ca,
                updated_at: ua,
                expires_at: ea,
                version: ver,
                owner: Some(owner),
            });
        }
        Ok(entries)
    }

    /// Regular 영역으로 import. `replace=true` 면 기존 key 덮어쓰기 (CAS 무시),
    /// `false` 면 충돌 시 건너뜀. 반환: 적용된 (created+updated) 갯수, skipped 갯수.
    /// 호출자 owner 가 caller_owner — _host 면 모든 row 변경 가능, plugin 이면 자기
    /// 영역만. 충돌 시 권한 위반은 `OwnedByOther`.
    pub fn import_regular(
        &mut self,
        caller_owner: &str,
        entries: &[MemoryEntry],
        replace: bool,
    ) -> Result<ImportStats> {
        validate_owner(caller_owner)?;
        let mut applied = 0u64;
        let mut skipped = 0u64;
        for e in entries {
            validate_key(&e.key).map_err(MemoryError::InvalidKey)?;
            let scope = Scope::parse(&e.scope).map_err(MemoryError::InvalidScope)?;
            let exists_now = self.get(&scope, &e.key)?.is_some();
            if exists_now && !replace {
                skipped += 1;
                continue;
            }
            self.put(
                caller_owner,
                &scope,
                &e.key,
                &e.value,
                &PutOpts {
                    expires_at: e.expires_at,
                    cas: None,
                },
            )?;
            applied += 1;
        }
        Ok(ImportStats { applied, skipped })
    }

    // ============================================================
    // Secret memory: plugin 별 사전 분할 (`owner` PK 일부)
    // ============================================================

    /// Secret put. owner 차원으로 자동 분리되므로 다른 plugin 영역과 충돌 없음.
    /// Value 는 평문 BLOB 그대로 저장 — 보호 수준은 plugin 간 IPC 격리까지.
    pub fn put_secret(
        &mut self,
        owner: &str,
        scope: &Scope,
        key: &str,
        value: &MemoryValue,
        opts: &PutOpts,
    ) -> Result<u64> {
        validate_owner(owner)?;
        validate_key(key).map_err(MemoryError::InvalidKey)?;
        let bytes = serialize_value(value)?;
        self.check_entry_size(bytes.len())?;
        let scope_token = scope.as_token();
        let now = unix_ms_now();
        let content_type = value.content_type();

        let tx = self.conn.transaction()?;
        let existing: Option<(u64, i64)> = tx
            .query_row(
                "SELECT version, LENGTH(value) FROM memory_secret
                 WHERE owner=?1 AND scope=?2 AND key=?3",
                params![owner, &scope_token, key],
                |r| Ok((r.get::<_, i64>(0)? as u64, r.get::<_, i64>(1)?)),
            )
            .optional()?;

        // Quota: per-owner secret 한도. 평문 byte 기준.
        let existing_size = existing.as_ref().map(|(_, sz)| *sz).unwrap_or(0);
        let current_used: i64 = tx.query_row(
            "SELECT COALESCE(SUM(LENGTH(value)), 0) FROM memory_secret WHERE owner=?1",
            params![owner],
            |r| r.get(0),
        )?;
        let projected = current_used - existing_size + bytes.len() as i64;
        if (projected as u64) > self.config.secret_quota_per_owner_bytes {
            return Err(MemoryError::QuotaExceeded {
                area: MemoryArea::Secret,
                used: projected.max(0) as u64,
                limit: self.config.secret_quota_per_owner_bytes,
            });
        }

        let new_version = match (existing, opts.cas) {
            (None, Some(expected)) => {
                return Err(MemoryError::CasConflict {
                    expected,
                    actual: 0,
                });
            }
            (Some((actual, _)), Some(expected)) if actual != expected => {
                return Err(MemoryError::CasConflict { expected, actual });
            }
            (Some((actual, _)), _) => {
                let new_v = actual + 1;
                tx.execute(
                    "UPDATE memory_secret SET value=?1, content_type=?2, updated_at=?3,
                                              expires_at=?4, version=?5
                     WHERE owner=?6 AND scope=?7 AND key=?8",
                    params![
                        &bytes,
                        content_type,
                        now,
                        opts.expires_at,
                        new_v as i64,
                        owner,
                        &scope_token,
                        key
                    ],
                )?;
                new_v
            }
            (None, _) => {
                tx.execute(
                    "INSERT INTO memory_secret
                       (owner, scope, key, value, content_type, created_at, updated_at,
                        expires_at, version)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6, ?7, 1)",
                    params![
                        owner,
                        &scope_token,
                        key,
                        &bytes,
                        content_type,
                        now,
                        opts.expires_at
                    ],
                )?;
                1
            }
        };

        let commit_started = std::time::Instant::now();
        tx.commit()?;
        self.db_latency.record_commit(commit_started.elapsed());
        Ok(new_version)
    }

    /// Secret 조회. 응답의 owner 필드는 None이다.
    pub fn get_secret(&self, owner: &str, scope: &Scope, key: &str) -> Result<Option<MemoryEntry>> {
        validate_owner(owner)?;
        validate_key(key).map_err(MemoryError::InvalidKey)?;
        let scope_token = scope.as_token();
        let now = unix_ms_now();

        let row = self
            .conn
            .query_row(
                "SELECT value, content_type, created_at, updated_at, expires_at, version
                 FROM memory_secret WHERE owner=?1 AND scope=?2 AND key=?3",
                params![owner, &scope_token, key],
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
            owner: None,
        }))
    }

    pub fn exists_secret(&self, owner: &str, scope: &Scope, key: &str) -> Result<bool> {
        Ok(self.get_secret(owner, scope, key)?.is_some())
    }

    pub fn delete_secret(
        &mut self,
        owner: &str,
        scope: &Scope,
        key: &str,
        cas: Option<u64>,
    ) -> Result<()> {
        validate_owner(owner)?;
        validate_key(key).map_err(MemoryError::InvalidKey)?;
        let scope_token = scope.as_token();

        let tx = self.conn.transaction()?;
        let existing: Option<u64> = tx
            .query_row(
                "SELECT version FROM memory_secret
                 WHERE owner=?1 AND scope=?2 AND key=?3",
                params![owner, &scope_token, key],
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
            "DELETE FROM memory_secret WHERE owner=?1 AND scope=?2 AND key=?3",
            params![owner, &scope_token, key],
        )?;
        let commit_started = std::time::Instant::now();
        tx.commit()?;
        self.db_latency.record_commit(commit_started.elapsed());
        Ok(())
    }

    pub fn list_secret(
        &self,
        owner: &str,
        scope: &Scope,
        opts: &ListOpts,
    ) -> Result<Vec<MemoryEntry>> {
        validate_owner(owner)?;
        let scope_token = scope.as_token();
        let now = unix_ms_now();
        let limit = opts.limit.unwrap_or(usize::MAX) as i64;
        let offset = opts.offset.unwrap_or(0) as i64;

        let mut sql = String::from(
            "SELECT key, value, content_type, created_at, updated_at, expires_at, version
             FROM memory_secret
             WHERE owner=? AND scope=? AND (expires_at IS NULL OR expires_at > ?)",
        );
        let mut binds: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        binds.push(Box::new(owner.to_string()));
        binds.push(Box::new(scope_token.clone()));
        binds.push(Box::new(now));

        if let Some(prefix) = &opts.prefix {
            sql.push_str(" AND key LIKE ? ESCAPE '\\'");
            binds.push(Box::new(format!("{}%", escape_like(prefix))));
        }
        if let Some(since) = opts.since {
            sql.push_str(" AND updated_at >= ?");
            binds.push(Box::new(since));
        }
        if let Some(until) = opts.until {
            sql.push_str(" AND updated_at < ?");
            binds.push(Box::new(until));
        }
        sql.push_str(" ORDER BY key ASC LIMIT ? OFFSET ?");
        binds.push(Box::new(limit));
        binds.push(Box::new(offset));

        let mut stmt = self.conn.prepare(&sql)?;
        let params_iter: Vec<&dyn rusqlite::ToSql> = binds.iter().map(|b| b.as_ref()).collect();
        type SecretRow = (String, Vec<u8>, String, i64, i64, Option<i64>, u64);
        let map_row = |r: &rusqlite::Row<'_>| -> rusqlite::Result<SecretRow> {
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

        let mut entries = Vec::new();
        let rows = stmt.query_map(params_iter.as_slice(), map_row)?;
        for row in rows {
            let (entry_key, bytes, ct, ca, ua, ea, ver) = row?;
            entries.push(MemoryEntry {
                scope: scope_token.clone(),
                key: entry_key,
                value: MemoryValue::from_db(&ct, bytes)?,
                created_at: ca,
                updated_at: ua,
                expires_at: ea,
                version: ver,
                owner: None,
            });
        }
        Ok(entries)
    }

    pub fn count_secret(&self, owner: &str, scope: &Scope, prefix: Option<&str>) -> Result<u64> {
        validate_owner(owner)?;
        let scope_token = scope.as_token();
        let now = unix_ms_now();
        let n: i64 = if let Some(p) = prefix {
            let like = format!("{}%", escape_like(p));
            self.conn.query_row(
                "SELECT COUNT(*) FROM memory_secret
                 WHERE owner=?1 AND scope=?2 AND (expires_at IS NULL OR expires_at > ?3)
                   AND key LIKE ?4 ESCAPE '\\'",
                params![owner, &scope_token, now, like],
                |r| r.get(0),
            )?
        } else {
            self.conn.query_row(
                "SELECT COUNT(*) FROM memory_secret
                 WHERE owner=?1 AND scope=?2 AND (expires_at IS NULL OR expires_at > ?3)",
                params![owner, &scope_token, now],
                |r| r.get(0),
            )?
        };
        Ok(n as u64)
    }

    pub fn scopes_secret(&self, owner: &str) -> Result<Vec<String>> {
        validate_owner(owner)?;
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT scope FROM memory_secret WHERE owner=?1 ORDER BY scope ASC",
        )?;
        let rows = stmt.query_map(params![owner], |r| r.get::<_, String>(0))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn stats_secret(&self, owner: &str, scope: Option<&Scope>) -> Result<MemoryStats> {
        validate_owner(owner)?;
        let now = unix_ms_now();
        if let Some(s) = scope {
            let token = s.as_token();
            let (count, bytes): (i64, i64) = self.conn.query_row(
                "SELECT COUNT(*), COALESCE(SUM(LENGTH(value)), 0) FROM memory_secret
                 WHERE owner=?1 AND scope=?2 AND (expires_at IS NULL OR expires_at > ?3)",
                params![owner, &token, now],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            Ok(MemoryStats {
                scope: Some(token),
                entries: count as u64,
                bytes: bytes as u64,
            })
        } else {
            let (count, bytes): (i64, i64) = self.conn.query_row(
                "SELECT COUNT(*), COALESCE(SUM(LENGTH(value)), 0) FROM memory_secret
                 WHERE owner=?1 AND (expires_at IS NULL OR expires_at > ?2)",
                params![owner, now],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            Ok(MemoryStats {
                scope: None,
                entries: count as u64,
                bytes: bytes as u64,
            })
        }
    }

    // ============================================================
    // GC / Maintenance
    // ============================================================

    /// 만료된 entry (regular + secret) 일괄 DELETE. 반환: 삭제된 row 수.
    /// read 경로는 항상 `expires_at` 필터를 적용하므로 만료 entry 가 노출되진
    /// 않지만, 디스크에 row 가 남아 quota 와 파일 크기를 부풀린다. 호스트는
    /// 주기적으로 또는 `memory.gc` 명령으로 이 함수를 호출해 청소한다.
    pub fn purge_expired(&mut self) -> Result<PurgeStats> {
        let now = unix_ms_now();
        // Regular: 발화할 key 목록을 먼저 수집한 뒤 delete (secret 은 발화하지 않으므로
        // 단순 count 만 필요).
        let expired_regular: Vec<(String, String)> = {
            let mut stmt = self.conn.prepare(
                "SELECT scope, key FROM memory
                 WHERE expires_at IS NOT NULL AND expires_at <= ?1",
            )?;
            let rows = stmt.query_map(params![now], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        let regular = self.conn.execute(
            "DELETE FROM memory WHERE expires_at IS NOT NULL AND expires_at <= ?1",
            params![now],
        )?;
        let secret = self.conn.execute(
            "DELETE FROM memory_secret WHERE expires_at IS NOT NULL AND expires_at <= ?1",
            params![now],
        )?;
        for (scope, key) in expired_regular {
            self.pending_changes.push(MemoryChange {
                scope,
                key,
                kind: MemoryChangeKind::Expired,
                version: None,
            });
        }
        self.regular_used_bytes = Self::scan_regular_used(&self.conn);
        Ok(PurgeStats {
            regular: regular as u64,
            secret: secret as u64,
        })
    }

    /// 특정 scope 의 모든 entry (regular + secret 양쪽) 삭제. surface/window/
    /// workspace 가 닫힐 때 호스트가 호출해 해당 수명에 묶인 키를 정리한다.
    pub fn purge_scope(&mut self, scope: &Scope) -> Result<PurgeStats> {
        let token = scope.as_token();
        // Regular keys 발화용 수집 (secret 은 발화 안 함).
        let cleared_keys: Vec<String> = {
            let mut stmt = self.conn.prepare("SELECT key FROM memory WHERE scope=?1")?;
            let rows = stmt.query_map(params![&token], |r| r.get::<_, String>(0))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        let regular = self
            .conn
            .execute("DELETE FROM memory WHERE scope=?1", params![&token])?;
        let secret = self
            .conn
            .execute("DELETE FROM memory_secret WHERE scope=?1", params![&token])?;
        for key in cleared_keys {
            self.pending_changes.push(MemoryChange {
                scope: token.clone(),
                key,
                kind: MemoryChangeKind::Deleted,
                version: None,
            });
        }
        self.regular_used_bytes = Self::scan_regular_used(&self.conn);
        Ok(PurgeStats {
            regular: regular as u64,
            secret: secret as u64,
        })
    }

    /// Pending change buffer 를 비우고 반환. 호스트가 매 tick 호출해
    /// Event Bus 의 `memory.changed` 로 broadcast.
    pub fn take_pending_changes(&mut self) -> Vec<MemoryChange> {
        std::mem::take(&mut self.pending_changes)
    }

    /// prefix로 시작하는 로그 키를 내림차순으로 정렬해 최근 keep_recent개만 남긴다.
    /// 키가 0으로 채운 시각을 포함해 문자열 순서가 시간 순서와 같은 로그용이다.
    /// 변경 이벤트 없이 삭제하며 삭제 수를 반환한다. scope는 구분하지 않는다.
    /// prefix의 LIKE 특수문자는 이스케이프한다.
    pub fn prune_prefix_keep_recent(&mut self, prefix: &str, keep_recent: u64) -> Result<u64> {
        let like = format!(
            "{}%",
            prefix
                .replace('\\', "\\\\")
                .replace('_', "\\_")
                .replace('%', "\\%")
        );
        // OFFSET=keep_recent 위치(0-based)의 키 = "최신에서 N+1번째" = 첫 삭제 대상.
        // 그 키 이하(`<=`)를 모두 삭제 → 최신 N개만 남는다. 매칭 행 ≤ keep_recent 면
        // OFFSET 이 범위를 벗어나 cutoff=NULL → `key <= NULL` = NULL → 삭제 0 (no-op).
        let n = self.conn.execute(
            "DELETE FROM memory WHERE key LIKE ?1 ESCAPE '\\' AND key <= (
                 SELECT key FROM memory WHERE key LIKE ?1 ESCAPE '\\'
                 ORDER BY key DESC LIMIT 1 OFFSET ?2
             )",
            // u64를 그대로 i64로 줄여 음수가 되면 OFFSET이 0처럼 적용될 수 있다.
            params![like, keep_recent.min(i64::MAX as u64) as i64],
        )?;
        if n > 0 {
            self.regular_used_bytes = Self::scan_regular_used(&self.conn);
        }
        Ok(n as u64)
    }

    /// prefix 뒤 13자리 시각이 cutoff_ms 미만인 로그를 이벤트 없이 삭제한다.
    /// 개수 제한과 함께 사용하며 삭제 수를 반환한다. prefix의 LIKE 특수문자는 이스케이프한다.
    pub fn prune_prefix_older_than(&mut self, prefix: &str, cutoff_ms: u64) -> Result<u64> {
        let like = format!(
            "{}%",
            prefix
                .replace('\\', "\\\\")
                .replace('_', "\\_")
                .replace('%', "\\%")
        );
        // 같은 시각에 .seq가 붙은 키는 경계보다 커서 남는다.
        let boundary = format!("{prefix}{cutoff_ms:013}");
        let n = self.conn.execute(
            "DELETE FROM memory WHERE key LIKE ?1 ESCAPE '\\' AND key < ?2",
            params![like, boundary],
        )?;
        if n > 0 {
            self.regular_used_bytes = Self::scan_regular_used(&self.conn);
        }
        Ok(n as u64)
    }

    /// WAL을 checkpoint하고 0바이트로 줄인다.
    /// busy로 끝나면 false이며 파일이 남을 수 있다. 다른 SQLite 오류는 Err로 반환한다.
    pub fn checkpoint_truncate(&mut self) -> Result<bool> {
        // (busy, log_pages, checkpointed_pages) 한 행을 돌려준다.
        let started = std::time::Instant::now();
        let busy: i64 = self
            .conn
            .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| r.get(0))?;
        self.db_latency
            .record_checkpoint(started.elapsed(), busy == 0);
        Ok(busy == 0)
    }

    /// freelist 가 `min_free_pages` 이상이면 `VACUUM` 으로 파일을 압축해 디스크를
    /// 회수한다(대량 prune 직후 1회용). VACUUM 은 파일 전체를 재작성하므로 평소엔
    /// 호출하지 않는다. 압축 수행 시 true.
    pub fn vacuum_if_fragmented(&mut self, min_free_pages: i64) -> Result<bool> {
        let free: i64 = self
            .conn
            .query_row("PRAGMA freelist_count", [], |r| r.get(0))
            .unwrap_or(0);
        if free < min_free_pages {
            return Ok(false);
        }
        self.conn.execute_batch("VACUUM")?;
        Ok(true)
    }
}

/// GC 결과. regular/secret 영역별 삭제 row 수.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PurgeStats {
    pub regular: u64,
    pub secret: u64,
}

/// Import 결과. CAS 무시 (replace=true) 또는 충돌 시 skip (replace=false).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportStats {
    /// 적용된 entry 갯수 (신규 + 갱신 합산).
    pub applied: u64,
    /// 기존 key 존재 + `replace=false` 로 건너뛴 갯수.
    pub skipped: u64,
}

/// JSON 값에서 `"a.b.c"` 형식 dot path 로 nested object 필드 lookup.
/// 배열 인덱스는 지원하지 않는다.
pub(crate) fn lookup_dot_path<'a>(
    value: &'a serde_json::Value,
    path: &str,
) -> Option<&'a serde_json::Value> {
    let mut cur = value;
    for seg in path.split('.') {
        if seg.is_empty() {
            return None;
        }
        let obj = cur.as_object()?;
        cur = obj.get(seg)?;
    }
    Some(cur)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryStats {
    pub scope: Option<String>,
    pub entries: u64,
    pub bytes: u64,
}

/// Init/open 실패. `MemoryError` 와 분리 — 호출자가 사용자 친화 메시지를 띄울
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
            MemoryInitError::PermissionDenied(p) => {
                write!(f, "permission denied: {}", p.display())
            }
            MemoryInitError::Busy(p) => write!(f, "memory.db busy: {}", p.display()),
            MemoryInitError::DiskFull => write!(f, "disk full"),
            MemoryInitError::Corrupt(p) => write!(f, "memory.db corrupted: {}", p.display()),
            MemoryInitError::SchemaMismatch { expected, found } => {
                write!(
                    f,
                    "memory.db schema mismatch (expected {expected}, found {found})"
                )
            }
            MemoryInitError::Other(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for MemoryInitError {}

fn classify_io(err: std::io::Error, path: &Path) -> MemoryInitError {
    match err.kind() {
        std::io::ErrorKind::PermissionDenied => {
            MemoryInitError::PermissionDenied(path.to_path_buf())
        }
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

/// StorageFailure와 같은 분류를 쓴다. 별도 초기화 안내가 없는 Io는 Other로 변환한다.
fn classify_sql(err: rusqlite::Error, path: &Path) -> MemoryInitError {
    match StorageFailure::classify(&err) {
        StorageFailure::Busy => MemoryInitError::Busy(path.to_path_buf()),
        StorageFailure::Corrupt => MemoryInitError::Corrupt(path.to_path_buf()),
        StorageFailure::DiskFull => MemoryInitError::DiskFull,
        StorageFailure::PermissionDenied => MemoryInitError::PermissionDenied(path.to_path_buf()),
        StorageFailure::Io | StorageFailure::Other => {
            MemoryInitError::Other(format!("{}: {err}", path.display()))
        }
    }
}

/// `memory.db` 기본 경로 (`tasty_home()/memory.db`). 홈 디렉터리 미확인 시 `None`.
pub fn default_db_path() -> Option<PathBuf> {
    tasty_utils::path::tasty_home().map(|d| d.join("memory.db"))
}

/// 앱 시작 시 1회. Settings.memory 에서 도출한 [`MemoryConfig`] 로 연다.
/// 새 `Arc<Mutex<MemoryStore>>` 를 반환 — caller (host bin 의 boot) 가 Core 에 inject.
pub fn init_with_config(
    config: MemoryConfig,
) -> std::result::Result<Arc<Mutex<MemoryStore>>, MemoryInitError> {
    let path = default_db_path().ok_or(MemoryInitError::HomeDirMissing)?;
    let store = MemoryStore::open_with_config(&path, config)?;
    tracing::info!("opened memory.db at {}", path.display());
    Ok(Arc::new(Mutex::new(store)))
}

pub(crate) fn unix_ms_now() -> i64 {
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

fn validate_owner(owner: &str) -> Result<()> {
    if owner.is_empty() {
        return Err(MemoryError::InvalidOwner("empty".into()));
    }
    if owner.len() > 256 {
        return Err(MemoryError::InvalidOwner(format!(
            "too long: {} > 256",
            owner.len()
        )));
    }
    Ok(())
}

/// `_host` 는 모든 entry 를 수정할 수 있는 root, plugin owner 는 자기 entry 만 수정 가능.
fn owner_can_modify(caller_owner: &str, existing_owner: &str) -> bool {
    caller_owner == HOST_OWNER || caller_owner == existing_owner
}

fn serialize_value(value: &MemoryValue) -> Result<Vec<u8>> {
    value
        .to_bytes()
        .map_err(|e| MemoryError::InvalidContentType(e.to_string()))
}

#[cfg(test)]
mod tests;
