//! 단일 schema 모델.
//!
//! 0.4 fresh-start 정책: 하위 호환을 위한 마이그레이션 체인은 제거됐다.
//! 신규 DB(`user_version == 0`)는 `SCHEMA_SQL`을 한 번 적용하고 `user_version`을
//! `SCHEMA_VERSION`으로 박는다. 이미 같은 버전이면 no-op. 다른 버전이면
//! `SchemaMismatch` 에러를 반환해서 호출자가 사용자에게 안내한 뒤 종료한다.

use rusqlite::Connection;

pub const SCHEMA_VERSION: u32 = 1;

const SCHEMA_SQL: &str = r#"
    CREATE TABLE IF NOT EXISTS meta (
        key TEXT PRIMARY KEY,
        value TEXT NOT NULL
    );

    CREATE TABLE IF NOT EXISTS recent_markdown (
        path TEXT PRIMARY KEY,
        opened_at INTEGER NOT NULL
    );
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
                "schema version mismatch (expected {expected}, found {found})"
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
        tracing::info!("state.db schema initialized at v{SCHEMA_VERSION}");
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

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN ('meta','recent_markdown')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);

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
        let ver: u32 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(ver, SCHEMA_VERSION);
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

    #[test]
    fn older_user_version_is_mismatch() {
        // 0.4 이전 DB가 user_version=1로 박혀 있었다면 SCHEMA_VERSION이 같아 OK가 맞다.
        // 하지만 명시적으로 다른 값을 갖고 있으면 mismatch.
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "user_version", 2u32).unwrap();
        let err = ensure_schema(&mut conn).unwrap_err();
        assert!(matches!(
            err,
            DbSchemaError::SchemaMismatch { found: 2, .. }
        ));
    }
}
