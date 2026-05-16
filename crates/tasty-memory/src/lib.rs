//! 에이전트 메모리 저장소 (`~/.tasty/memory.db`).
//!
//! 에이전트와 plugin 이 작업 도중 누적·검색·공유하는 영속 키-값. 같은 SQLite
//! 파일에 두 영역이 공존한다:
//!
//! - **Regular** (`memory.*`): 공유 네임스페이스. 모든 plugin 이 모든 entry 를
//!   읽지만, 갱신·삭제는 `owner` 본인 또는 `_host` (CLI / 사용자) 만 가능.
//! - **Secret** (`memory.secret.*`): plugin 별 사전 분할. owner 가 PK 일부라
//!   다른 plugin 영역은 IPC 표면에서 개념 자체가 존재하지 않는다.
//!
//! `owner` 는 호스트가 [`CallerContext`] 로부터 자동 도출하는 값이다 — plugin 이
//! 인자로 넘길 수 없고 본 크레이트에서는 **`&str` 으로 받기만 한다**. caller 가
//! `_host` 인지 plugin 인지는 호출자 (호스트 IPC 라우터) 책임.
//!
//! ## 동기 모델
//!
//! Tasty 본 바이너리는 winit 이벤트 루프 + sync 코드 베이스다 (tokio 사용 안 함).
//! `MemoryStore` 는 `OnceLock<Mutex<MemoryStore>>` 싱글톤 + `with_store(...)`
//! 동기 콜백. IPC dispatch 는 메인 스레드에서 순차 호출되고, plugin process 호출도
//! 별도 스레드의 mpsc 경로를 거쳐 결국 메인에서 처리되므로 단일 mutex 로 충분.

mod migrations;
mod scope;

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, ErrorCode, OptionalExtension, params};
use serde::{Deserialize, Serialize};

pub use migrations::{DbSchemaError, SCHEMA_VERSION};
pub use scope::{Scope, validate_key};

/// 단일 값 최대 크기 fallback (1 MiB). config 미주입 시 사용. 실제 cap 은
/// `MemoryConfig::entry_max_bytes` 가 결정한다.
pub const MAX_VALUE_BYTES: usize = 1024 * 1024;

/// Local caller (CLI / 사용자 / 호스트 내부) 의 owner sentinel. plugin id 의
/// reverse-DNS 규칙과 충돌하지 않도록 underscore prefix.
pub const HOST_OWNER: &str = "_host";

/// `MemoryStore` 의 정책 설정. 모든 cap 은 bytes 단위 (config 에서는 MiB 단위로
/// 들어와 호출자가 변환). default 는 design doc 의 fallback 값 (1 MiB / 10 MiB /
/// 1 GiB / plaintext disabled).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryConfig {
    pub entry_max_bytes: u64,
    pub secret_quota_per_owner_bytes: u64,
    pub regular_quota_total_bytes: u64,
    /// Keyring 부재 환경에서 secret 영역 평문 폴백 허용 여부 (Stage C 에서 사용).
    pub allow_plaintext_secret: bool,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            entry_max_bytes: 1024 * 1024,
            secret_quota_per_owner_bytes: 10 * 1024 * 1024,
            regular_quota_total_bytes: 1024 * 1024 * 1024,
            allow_plaintext_secret: false,
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
    #[error("secret memory unavailable (keyring not accessible)")]
    SecretUnavailable,
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
/// `owner` 는 regular 응답에서는 `Some`, secret 응답에서는 `None` (추상화 누수 방지).
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
}

/// MemoryStore. 디스크 파일을 단독으로 열어 mutex 보호. clone 불가.
pub struct MemoryStore {
    conn: Connection,
    config: MemoryConfig,
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

    /// 인메모리 (테스트 전용).
    #[cfg(test)]
    pub fn open_in_memory() -> std::result::Result<Self, MemoryInitError> {
        let conn =
            Connection::open_in_memory().map_err(|e| classify_sql(e, Path::new(":memory:")))?;
        Self::prepare(conn, Path::new(":memory:"), MemoryConfig::default())
    }

    #[cfg(test)]
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
        conn.pragma_update(None, "journal_mode", "WAL").ok();
        conn.pragma_update(None, "synchronous", "NORMAL").ok();
        conn.pragma_update(None, "foreign_keys", "ON").ok();
        migrations::ensure_schema(&mut conn).map_err(|e| match e {
            DbSchemaError::SchemaMismatch { expected, found } => {
                MemoryInitError::SchemaMismatch { expected, found }
            }
            DbSchemaError::Sql(e) => classify_sql(e, path),
        })?;
        Ok(Self { conn, config })
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

        // Quota: 추가될 byte 가 regular total 한도를 넘기지 않는지.
        let existing_size = existing.as_ref().map(|(_, _, sz)| *sz as i64).unwrap_or(0);
        let current_used: i64 = tx.query_row(
            "SELECT COALESCE(SUM(LENGTH(value)), 0) FROM memory",
            [],
            |r| r.get(0),
        )?;
        let projected = current_used - existing_size + bytes.len() as i64;
        if (projected as u64) > self.config.regular_quota_total_bytes {
            return Err(MemoryError::QuotaExceeded {
                area: MemoryArea::Regular,
                used: projected.max(0) as u64,
                limit: self.config.regular_quota_total_bytes,
            });
        }

        let new_version = match (existing, opts.cas) {
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
                new_v
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
                1
            }
        };

        tx.commit()?;
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
        let existing: Option<(u64, String)> = tx
            .query_row(
                "SELECT version, owner FROM memory WHERE scope=?1 AND key=?2",
                params![&scope_token, key],
                |r| Ok((r.get::<_, i64>(0)? as u64, r.get::<_, String>(1)?)),
            )
            .optional()?;

        match (existing, cas) {
            (None, _) => {
                return Err(MemoryError::NotFound {
                    scope: scope_token,
                    key: key.to_string(),
                });
            }
            (Some((actual, _)), Some(expected)) if actual != expected => {
                return Err(MemoryError::CasConflict { expected, actual });
            }
            (Some((_, existing_owner)), _) if !owner_can_modify(owner, &existing_owner) => {
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
        tx.commit()?;
        Ok(())
    }

    /// 스코프 내 키 리스트. 만료 제외, prefix/limit 옵션. 응답 entry 에 owner 포함.
    pub fn list(&self, scope: &Scope, opts: &ListOpts) -> Result<Vec<MemoryEntry>> {
        let scope_token = scope.as_token();
        let now = unix_ms_now();
        let limit = opts.limit.unwrap_or(usize::MAX) as i64;

        let mut sql = String::from(
            "SELECT key, value, content_type, created_at, updated_at, expires_at, version, owner
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

        let push_row =
            |entries: &mut Vec<MemoryEntry>,
             row: rusqlite::Result<RegularRow>|
             -> Result<()> {
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
                Ok(())
            };

        if let Some(prefix) = &opts.prefix {
            let like = format!("{}%", escape_like(prefix));
            let rows = stmt.query_map(params![&scope_token, now, like, limit], map_row)?;
            for row in rows {
                push_row(&mut entries, row)?;
            }
        } else {
            let rows = stmt.query_map(params![&scope_token, now, limit], map_row)?;
            for row in rows {
                push_row(&mut entries, row)?;
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

    // ============================================================
    // Secret memory: plugin 별 사전 분할 (`owner` PK 일부)
    // ============================================================

    /// Secret put. owner 차원으로 자동 분리되므로 다른 plugin 영역과 충돌 없음.
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

        // Quota: per-owner secret 한도.
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

        tx.commit()?;
        Ok(new_version)
    }

    /// Secret get. 응답 entry 의 `owner` 필드는 `None` 으로 두어 plugin 에게
    /// 추상화 누수를 만들지 않는다.
    pub fn get_secret(
        &self,
        owner: &str,
        scope: &Scope,
        key: &str,
    ) -> Result<Option<MemoryEntry>> {
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
        tx.commit()?;
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

        let mut sql = String::from(
            "SELECT key, value, content_type, created_at, updated_at, expires_at, version
             FROM memory_secret
             WHERE owner=?1 AND scope=?2 AND (expires_at IS NULL OR expires_at > ?3)",
        );
        if opts.prefix.is_some() {
            sql.push_str(" AND key LIKE ?4 ESCAPE '\\'");
        }
        sql.push_str(" ORDER BY key ASC LIMIT ?");
        sql.push_str(if opts.prefix.is_some() { "5" } else { "4" });

        let mut stmt = self.conn.prepare(&sql)?;
        let mut entries = Vec::new();
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

        let push_row =
            |entries: &mut Vec<MemoryEntry>,
             row: rusqlite::Result<SecretRow>|
             -> Result<()> {
                let (key, bytes, ct, ca, ua, ea, ver) = row?;
                entries.push(MemoryEntry {
                    scope: scope_token.clone(),
                    key,
                    value: MemoryValue::from_db(&ct, bytes)?,
                    created_at: ca,
                    updated_at: ua,
                    expires_at: ea,
                    version: ver,
                    owner: None,
                });
                Ok(())
            };

        if let Some(prefix) = &opts.prefix {
            let like = format!("{}%", escape_like(prefix));
            let rows = stmt.query_map(params![owner, &scope_token, now, like, limit], map_row)?;
            for row in rows {
                push_row(&mut entries, row)?;
            }
        } else {
            let rows = stmt.query_map(params![owner, &scope_token, now, limit], map_row)?;
            for row in rows {
                push_row(&mut entries, row)?;
            }
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

/// 앱 시작 시 1회. 기본 config 로 연다.
pub fn init() -> std::result::Result<(), MemoryInitError> {
    init_with_config(MemoryConfig::default())
}

/// 앱 시작 시 1회. Settings.memory 에서 도출한 [`MemoryConfig`] 로 연다.
/// 이미 초기화된 상태면 no-op (덮어쓰기 안 함).
pub fn init_with_config(config: MemoryConfig) -> std::result::Result<(), MemoryInitError> {
    if STORE.get().is_some() {
        return Ok(());
    }
    let path = default_db_path().ok_or(MemoryInitError::HomeDirMissing)?;
    let store = MemoryStore::open_with_config(&path, config)?;
    tracing::info!("opened memory.db at {}", path.display());
    let _ = STORE.set(Mutex::new(store));
    Ok(())
}

/// 테스트: 이미 열린 store 를 싱글톤으로 등록.
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
mod tests {
    use super::*;

    fn store() -> MemoryStore {
        MemoryStore::open_in_memory().unwrap()
    }

    fn text(s: &str) -> MemoryValue {
        MemoryValue::Text(s.into())
    }

    const PLUGIN_A: &str = "com.tasty.a";
    const PLUGIN_B: &str = "com.tasty.b";

    // ---- Regular ----

    #[test]
    fn put_get_delete_roundtrip() {
        let mut s = store();
        let scope = Scope::Surface(1);

        let v1 = s
            .put(PLUGIN_A, &scope, "a", &text("hello"), &PutOpts::default())
            .unwrap();
        assert_eq!(v1, 1);

        let entry = s.get(&scope, "a").unwrap().unwrap();
        assert_eq!(entry.value, text("hello"));
        assert_eq!(entry.version, 1);
        assert_eq!(entry.owner.as_deref(), Some(PLUGIN_A));

        let v2 = s
            .put(PLUGIN_A, &scope, "a", &text("world"), &PutOpts::default())
            .unwrap();
        assert_eq!(v2, 2);

        s.delete(PLUGIN_A, &scope, "a", None).unwrap();
        assert!(s.get(&scope, "a").unwrap().is_none());
    }

    #[test]
    fn scopes_are_isolated() {
        let mut s = store();
        s.put(HOST_OWNER, &Scope::Surface(1), "k", &text("s1"), &PutOpts::default())
            .unwrap();
        s.put(HOST_OWNER, &Scope::Surface(2), "k", &text("s2"), &PutOpts::default())
            .unwrap();
        s.put(HOST_OWNER, &Scope::Global, "k", &text("g"), &PutOpts::default())
            .unwrap();
        assert_eq!(s.get(&Scope::Surface(1), "k").unwrap().unwrap().value, text("s1"));
        assert_eq!(s.get(&Scope::Surface(2), "k").unwrap().unwrap().value, text("s2"));
        assert_eq!(s.get(&Scope::Global, "k").unwrap().unwrap().value, text("g"));
    }

    #[test]
    fn cas_conflict_blocks_update() {
        let mut s = store();
        let scope = Scope::Workspace(1);
        s.put(PLUGIN_A, &scope, "k", &text("v1"), &PutOpts::default())
            .unwrap();

        let err = s
            .put(
                PLUGIN_A,
                &scope,
                "k",
                &text("v2"),
                &PutOpts {
                    cas: Some(99),
                    ..Default::default()
                },
            )
            .unwrap_err();
        assert!(matches!(
            err,
            MemoryError::CasConflict {
                actual: 1,
                expected: 99
            }
        ));

        s.put(
            PLUGIN_A,
            &scope,
            "k",
            &text("v2"),
            &PutOpts {
                cas: Some(1),
                ..Default::default()
            },
        )
        .unwrap();
    }

    #[test]
    fn regular_owned_by_other_on_update() {
        let mut s = store();
        let scope = Scope::Global;
        s.put(PLUGIN_A, &scope, "k", &text("v1"), &PutOpts::default())
            .unwrap();

        let err = s
            .put(PLUGIN_B, &scope, "k", &text("v2"), &PutOpts::default())
            .unwrap_err();
        let owner = match err {
            MemoryError::OwnedByOther { owner } => owner,
            other => panic!("expected OwnedByOther, got {other:?}"),
        };
        assert_eq!(owner, PLUGIN_A);

        // 원래 owner는 정상.
        s.put(PLUGIN_A, &scope, "k", &text("v3"), &PutOpts::default())
            .unwrap();
        // _host는 root로 통과.
        s.put(HOST_OWNER, &scope, "k", &text("v4"), &PutOpts::default())
            .unwrap();
    }

    #[test]
    fn regular_owned_by_other_on_delete() {
        let mut s = store();
        let scope = Scope::Global;
        s.put(PLUGIN_A, &scope, "k", &text("v1"), &PutOpts::default())
            .unwrap();

        let err = s.delete(PLUGIN_B, &scope, "k", None).unwrap_err();
        assert!(matches!(err, MemoryError::OwnedByOther { .. }));

        // _host는 root로 통과해 삭제 가능.
        s.delete(HOST_OWNER, &scope, "k", None).unwrap();
        assert!(s.get(&scope, "k").unwrap().is_none());
    }

    #[test]
    fn read_is_shared_across_callers() {
        let mut s = store();
        let scope = Scope::Global;
        s.put(PLUGIN_A, &scope, "k", &text("v"), &PutOpts::default())
            .unwrap();
        // Plugin B도 읽을 수 있고, 응답에 owner=PLUGIN_A가 보인다.
        let entry = s.get(&scope, "k").unwrap().unwrap();
        assert_eq!(entry.owner.as_deref(), Some(PLUGIN_A));

        let list = s.list(&scope, &ListOpts::default()).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].owner.as_deref(), Some(PLUGIN_A));
    }

    #[test]
    fn expired_keys_treated_as_missing() {
        let mut s = store();
        let scope = Scope::Surface(1);
        let past = unix_ms_now() - 1000;
        s.put(
            HOST_OWNER,
            &scope,
            "k",
            &text("v"),
            &PutOpts {
                expires_at: Some(past),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(s.get(&scope, "k").unwrap().is_none());
        assert_eq!(s.count(&scope, None).unwrap(), 0);
        assert!(s.list(&scope, &ListOpts::default()).unwrap().is_empty());
    }

    #[test]
    fn value_size_cap_enforced() {
        let mut s = store();
        let cap = s.config().entry_max_bytes as usize;
        let big = vec![0u8; cap + 1];
        let err = s
            .put(
                HOST_OWNER,
                &Scope::Global,
                "k",
                &MemoryValue::Binary(big),
                &PutOpts::default(),
            )
            .unwrap_err();
        assert!(matches!(err, MemoryError::ValueTooLarge { .. }));
    }

    #[test]
    fn entry_max_configurable() {
        let mut s = MemoryStore::open_in_memory_with_config(MemoryConfig {
            entry_max_bytes: 16,
            ..MemoryConfig::default()
        })
        .unwrap();
        s.put(HOST_OWNER, &Scope::Global, "k", &text("0123456789ab"), &PutOpts::default())
            .unwrap();
        let err = s
            .put(
                HOST_OWNER,
                &Scope::Global,
                "big",
                &text("0123456789abcdefghij"),
                &PutOpts::default(),
            )
            .unwrap_err();
        assert!(matches!(err, MemoryError::ValueTooLarge { actual, max } if max == 16 && actual == 20));
    }

    #[test]
    fn regular_quota_exceeded() {
        let mut s = MemoryStore::open_in_memory_with_config(MemoryConfig {
            entry_max_bytes: 1024,
            regular_quota_total_bytes: 20,
            ..MemoryConfig::default()
        })
        .unwrap();
        // 12 byte 저장: 합산 12 ≤ 20 통과.
        s.put(HOST_OWNER, &Scope::Global, "a", &text("0123456789ab"), &PutOpts::default())
            .unwrap();
        // 새 entry 12 byte 추가 시 projected=24 → 거부.
        let err = s
            .put(HOST_OWNER, &Scope::Global, "b", &text("0123456789ab"), &PutOpts::default())
            .unwrap_err();
        match err {
            MemoryError::QuotaExceeded { area, used, limit } => {
                assert_eq!(area, MemoryArea::Regular);
                assert_eq!(used, 24);
                assert_eq!(limit, 20);
            }
            other => panic!("expected QuotaExceeded, got {other:?}"),
        }
        // 기존 entry 의 in-place 갱신은 existing_size 만큼 차감 후 평가 → 같은 크기면 통과.
        s.put(HOST_OWNER, &Scope::Global, "a", &text("ABCDEFGHIJKL"), &PutOpts::default())
            .unwrap();
    }

    #[test]
    fn secret_quota_exceeded_per_owner() {
        let mut s = MemoryStore::open_in_memory_with_config(MemoryConfig {
            entry_max_bytes: 1024,
            secret_quota_per_owner_bytes: 20,
            ..MemoryConfig::default()
        })
        .unwrap();
        // Plugin A: 12 byte 통과.
        s.put_secret(PLUGIN_A, &Scope::Global, "a", &text("0123456789ab"), &PutOpts::default())
            .unwrap();
        // Plugin A 가 추가 12 byte → 24 거부.
        let err = s
            .put_secret(PLUGIN_A, &Scope::Global, "b", &text("0123456789ab"), &PutOpts::default())
            .unwrap_err();
        assert!(matches!(
            err,
            MemoryError::QuotaExceeded {
                area: MemoryArea::Secret,
                ..
            }
        ));
        // Plugin B 영역은 독립 — 동일 12 byte 가능.
        s.put_secret(PLUGIN_B, &Scope::Global, "a", &text("0123456789ab"), &PutOpts::default())
            .unwrap();
    }

    #[test]
    fn invalid_key_rejected() {
        let mut s = store();
        let err = s
            .put(HOST_OWNER, &Scope::Global, "BAD", &text("x"), &PutOpts::default())
            .unwrap_err();
        assert!(matches!(err, MemoryError::InvalidKey(_)));
    }

    #[test]
    fn invalid_owner_rejected() {
        let mut s = store();
        let err = s
            .put("", &Scope::Global, "k", &text("x"), &PutOpts::default())
            .unwrap_err();
        assert!(matches!(err, MemoryError::InvalidOwner(_)));
    }

    #[test]
    fn delete_missing_returns_not_found() {
        let mut s = store();
        let err = s.delete(HOST_OWNER, &Scope::Global, "ghost", None).unwrap_err();
        assert!(matches!(err, MemoryError::NotFound { .. }));
    }

    #[test]
    fn list_prefix_and_limit() {
        let mut s = store();
        let scope = Scope::Surface(1);
        for k in ["a.1", "a.2", "b.1", "b.2", "c.1"] {
            s.put(HOST_OWNER, &scope, k, &text(k), &PutOpts::default())
                .unwrap();
        }
        let all = s.list(&scope, &ListOpts::default()).unwrap();
        assert_eq!(all.len(), 5);
        let a_only = s
            .list(
                &scope,
                &ListOpts {
                    prefix: Some("a.".into()),
                    limit: None,
                },
            )
            .unwrap();
        assert_eq!(
            a_only.iter().map(|e| e.key.as_str()).collect::<Vec<_>>(),
            vec!["a.1", "a.2"]
        );
        let limited = s
            .list(
                &scope,
                &ListOpts {
                    prefix: None,
                    limit: Some(2),
                },
            )
            .unwrap();
        assert_eq!(limited.len(), 2);
        assert_eq!(s.count(&scope, None).unwrap(), 5);
        assert_eq!(s.count(&scope, Some("b.")).unwrap(), 2);
    }

    // ---- Secret ----

    #[test]
    fn secret_isolated_between_owners() {
        let mut s = store();
        let scope = Scope::Global;
        s.put_secret(PLUGIN_A, &scope, "tok", &text("A-token"), &PutOpts::default())
            .unwrap();
        s.put_secret(PLUGIN_B, &scope, "tok", &text("B-token"), &PutOpts::default())
            .unwrap();

        // 같은 (scope, key)지만 owner별로 분리 — Plugin A는 자기 값만 본다.
        let a = s.get_secret(PLUGIN_A, &scope, "tok").unwrap().unwrap();
        assert_eq!(a.value, text("A-token"));
        assert!(a.owner.is_none(), "secret 응답에는 owner 노출 금지");

        let b = s.get_secret(PLUGIN_B, &scope, "tok").unwrap().unwrap();
        assert_eq!(b.value, text("B-token"));

        // Plugin A가 자기 영역만 본다.
        let list_a = s.list_secret(PLUGIN_A, &scope, &ListOpts::default()).unwrap();
        assert_eq!(list_a.len(), 1);

        let scopes_a = s.scopes_secret(PLUGIN_A).unwrap();
        assert_eq!(scopes_a, vec!["global"]);
    }

    #[test]
    fn secret_delete_only_affects_owner() {
        let mut s = store();
        let scope = Scope::Workspace(1);
        s.put_secret(PLUGIN_A, &scope, "tok", &text("A"), &PutOpts::default())
            .unwrap();
        s.put_secret(PLUGIN_B, &scope, "tok", &text("B"), &PutOpts::default())
            .unwrap();
        s.delete_secret(PLUGIN_A, &scope, "tok", None).unwrap();
        assert!(s.get_secret(PLUGIN_A, &scope, "tok").unwrap().is_none());
        assert!(s.get_secret(PLUGIN_B, &scope, "tok").unwrap().is_some());
    }

    #[test]
    fn secret_cas_and_versioning() {
        let mut s = store();
        let scope = Scope::Global;
        let v1 = s
            .put_secret(PLUGIN_A, &scope, "k", &text("v1"), &PutOpts::default())
            .unwrap();
        assert_eq!(v1, 1);
        let v2 = s
            .put_secret(
                PLUGIN_A,
                &scope,
                "k",
                &text("v2"),
                &PutOpts {
                    cas: Some(1),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(v2, 2);
        let err = s
            .put_secret(
                PLUGIN_A,
                &scope,
                "k",
                &text("v3"),
                &PutOpts {
                    cas: Some(99),
                    ..Default::default()
                },
            )
            .unwrap_err();
        assert!(matches!(err, MemoryError::CasConflict { .. }));
    }

    #[test]
    fn secret_stats_per_owner() {
        let mut s = store();
        s.put_secret(PLUGIN_A, &Scope::Global, "a", &text("xx"), &PutOpts::default())
            .unwrap();
        s.put_secret(PLUGIN_B, &Scope::Global, "a", &text("zzzz"), &PutOpts::default())
            .unwrap();

        let a = s.stats_secret(PLUGIN_A, None).unwrap();
        assert_eq!(a.entries, 1);
        assert_eq!(a.bytes, 2);

        let b = s.stats_secret(PLUGIN_B, None).unwrap();
        assert_eq!(b.entries, 1);
        assert_eq!(b.bytes, 4);
    }
}
