//! `memory.db` 스키마.
//!
//! 0.x experimental 정책: 마이그레이션 체인을 누적하지 않고 single SCHEMA_SQL을
//! 적용한다. `user_version == 0`이면 신규 DB로 간주해 일괄 적용, 같은 버전이면
//! no-op, 다른 버전이면 `SchemaMismatch` 에러로 호출자에게 위임한다 (0.7 직전에
//! 최종 freeze 후 누적 migration으로 전환 예정).

use rusqlite::Connection;

pub const SCHEMA_VERSION: u32 = 1;

const SCHEMA_SQL: &str = r#"
    -- Regular memory. 모든 plugin이 모든 entry를 읽을 수 있고, owner 본인 (또는
    -- _host root)만 갱신·삭제할 수 있다.
    --   scope: 'global' | 'account:<userid>' | 'window:<id>' | 'workspace:<id>' | 'surface:<id>'
    --   key:   1..256 [a-z0-9._-]+
    --   value: 직렬화된 바이트열. content_type으로 해석 (application/json | text/plain | application/octet-stream).
    --   version: 낙관적 락(CAS). update마다 +1.
    --   owner:  caller로부터 호스트가 도장찍는 값. plugin id(reverse-DNS) 또는 '_host'.
    CREATE TABLE IF NOT EXISTS memory (
        scope TEXT NOT NULL,
        key TEXT NOT NULL,
        value BLOB NOT NULL,
        content_type TEXT NOT NULL,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL,
        expires_at INTEGER,
        version INTEGER NOT NULL DEFAULT 1,
        owner TEXT NOT NULL,
        PRIMARY KEY (scope, key)
    );

    -- TTL GC 스캔용 부분 인덱스. expires_at IS NULL 행은 인덱스에서 제외돼 비용 절감.
    CREATE INDEX IF NOT EXISTS idx_memory_expires
        ON memory(expires_at) WHERE expires_at IS NOT NULL;

    -- 스코프 prefix 스캔/리스트용 보조 인덱스.
    CREATE INDEX IF NOT EXISTS idx_memory_scope_key
        ON memory(scope, key);

    -- Owner 필터 (regular 영역에서 plugin uninstall 정리, owner-specific stats 등에서 사용).
    CREATE INDEX IF NOT EXISTS idx_memory_owner
        ON memory(owner);

    -- Secret memory. 각 plugin마다 자기 전용 영역 — owner가 PK 일부라 다른 plugin이
    -- 같은 (scope, key)를 충돌 없이 가질 수 있다. value blob은 평문 저장이며,
    -- 보호 약속은 IPC 표면에서의 owner 격리 한 가지 (자세한 위협 모델은 lib.rs / memory-system.md).
    CREATE TABLE IF NOT EXISTS memory_secret (
        owner TEXT NOT NULL,
        scope TEXT NOT NULL,
        key TEXT NOT NULL,
        value BLOB NOT NULL,
        content_type TEXT NOT NULL,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL,
        expires_at INTEGER,
        version INTEGER NOT NULL DEFAULT 1,
        PRIMARY KEY (owner, scope, key)
    );

    CREATE INDEX IF NOT EXISTS idx_memory_secret_expires
        ON memory_secret(expires_at) WHERE expires_at IS NOT NULL;

    -- Owner 별 quota 합산, list/scan 최적화.
    CREATE INDEX IF NOT EXISTS idx_memory_secret_owner
        ON memory_secret(owner);
"#;

#[derive(Debug)]
pub enum DbSchemaError {
    SchemaMismatch { expected: u32, found: u32 },
    Sql(rusqlite::Error),
}

impl std::fmt::Display for DbSchemaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DbSchemaError::SchemaMismatch { expected, found } => write!(
                f,
                "memory.db schema mismatch (expected {expected}, found {found})"
            ),
            DbSchemaError::Sql(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for DbSchemaError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            DbSchemaError::Sql(e) => Some(e),
            _ => None,
        }
    }
}

impl From<rusqlite::Error> for DbSchemaError {
    fn from(e: rusqlite::Error) -> Self {
        DbSchemaError::Sql(e)
    }
}

/// 새 DB라면 schema를 적용하고, 같은 버전이면 no-op, 다른 버전이면 mismatch.
pub fn ensure_schema(conn: &mut Connection) -> Result<(), DbSchemaError> {
    let current: u32 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;

    if current == 0 {
        let tx = conn.transaction()?;
        tx.execute_batch(SCHEMA_SQL)?;
        tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        tx.commit()?;
        tracing::info!("memory.db schema initialized at v{SCHEMA_VERSION}");
        return Ok(());
    }

    if current == SCHEMA_VERSION {
        return Ok(());
    }

    Err(DbSchemaError::SchemaMismatch {
        expected: SCHEMA_VERSION,
        found: current,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_db_initializes_schema() {
        let mut conn = Connection::open_in_memory().unwrap();
        ensure_schema(&mut conn).unwrap();

        for table in ["memory", "memory_secret"] {
            let n: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                    [table],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(n, 1, "missing table: {table}");
        }

        let ver: u32 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(ver, SCHEMA_VERSION);
    }

    #[test]
    fn second_call_is_noop() {
        let mut conn = Connection::open_in_memory().unwrap();
        ensure_schema(&mut conn).unwrap();
        ensure_schema(&mut conn).unwrap();
    }

    #[test]
    fn schema_mismatch_returns_specific_error() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "user_version", 999u32).unwrap();
        let err = ensure_schema(&mut conn).unwrap_err();
        match err {
            DbSchemaError::SchemaMismatch { expected, found } => {
                assert_eq!(expected, SCHEMA_VERSION);
                assert_eq!(found, 999);
            }
            _ => panic!("expected SchemaMismatch, got {err:?}"),
        }
    }
}
