//! 새 DB에는 현재 스키마를 만들고 다른 user_version은 오류로 거부한다.
//! 같은 버전도 tutorial_progress 생성문을 다시 실행한다. 기존 테이블 구조 전체를 검사하거나 고치지는 않는다.

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

    CREATE TABLE IF NOT EXISTS recent_files (
        kind TEXT NOT NULL,
        path TEXT NOT NULL,
        opened_at INTEGER NOT NULL,
        PRIMARY KEY(kind, path)
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

pub fn ensure_schema(conn: &mut Connection) -> Result<(), DbSchemaError> {
    let current: u32 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;

    if current == 0 {
        let tx = conn.transaction()?;
        tx.execute_batch(SCHEMA_SQL)?;
        tx.execute_batch(crate::store::tutorial_progress::SCHEMA)?;
        tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        tx.commit()?;
        tracing::info!("state.db schema initialized at v{SCHEMA_VERSION}");
        return Ok(());
    }

    if current == SCHEMA_VERSION {
        conn.execute_batch(crate::store::tutorial_progress::SCHEMA)?;
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
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN ('meta','recent_markdown','recent_files')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 3);

        let ver: u32 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(ver, SCHEMA_VERSION);
    }

    #[test]
    fn a_second_call_leaves_the_version_alone() {
        let mut conn = Connection::open_in_memory().unwrap();
        ensure_schema(&mut conn).unwrap();
        ensure_schema(&mut conn).unwrap();
        let ver: u32 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(ver, SCHEMA_VERSION);
    }

    /// 기존 버전 DB에 tutorial_progress 테이블이 없던 경우도 생성하는지 확인한다.
    #[test]
    fn an_existing_database_still_gets_the_tutorial_table() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_SQL).unwrap();
        conn.pragma_update(None, "user_version", SCHEMA_VERSION)
            .unwrap();

        ensure_schema(&mut conn).unwrap();

        let found: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='tutorial_progress'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            found, 1,
            "additive ensure did not reach an existing database"
        );
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
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "user_version", 2u32).unwrap();
        let err = ensure_schema(&mut conn).unwrap_err();
        assert!(matches!(
            err,
            DbSchemaError::SchemaMismatch { found: 2, .. }
        ));
    }
}
