//! 단일 schema 모델.
//!
//! 0.4 fresh-start 정책: 하위 호환을 위한 마이그레이션 체인은 제거됐다.
//! 신규 DB(`user_version == 0`)는 `SCHEMA_SQL`을 한 번 적용하고 `user_version`을
//! `SCHEMA_VERSION`으로 박는다. 다른 버전이면 `SchemaMismatch` 에러를 반환해서
//! 호출자가 사용자에게 안내한 뒤 종료한다.
//!
//! **이미 같은 버전이면 additive ensure 다 — no-op 이 아니다.** v1 이 나간 뒤에 생긴
//! 테이블은 버전을 안 올리고 그 갈래로만 기존 DB 에 닿으므로, 거기서 `SCHEMA_SQL` 뒤에
//! 붙는 `CREATE TABLE IF NOT EXISTS` 들이 다시 돌아야 한다. 그 갈래에 무엇을 넣어도
//! 되는 것은 아니다 — 규칙과 근거는 `docs/design/systems/storage.md` 의 "단일 schema
//! 모델" 절에 있고, `an_existing_database_still_gets_the_tutorial_table` 이 고정한다.
//! (`memory.db` 쪽은 같은 모양이되 그 갈래가 진짜 `no-op` 이다 — 섞어 읽지 마라.)

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

/// 새 DB라면 schema 를 적용하고, 같은 버전이면 **additive ensure**(`CREATE TABLE IF NOT
/// EXISTS` 만 다시 돌린다), 다른 버전이면 mismatch.
///
/// 같은-버전 갈래가 no-op 이 아니라는 것이 이 함수에서 제일 놓치기 쉬운 지점이다 —
/// 모듈 머리말 참조.
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

    /// 이름이 `second_call_is_noop` 이 아닌 이유: 두 번째 호출이 버전을 안 바꾸는 것은
    /// 참이지만 **그 갈래 자체는 no-op 이 아니다**(additive ensure). 식별자가 그 낱말을
    /// 들고 있으면 이 파일에서 그 오해를 지운 의미가 없어진다.
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

    /// 이미 `SCHEMA_VERSION` 인 DB 에도 `tutorial_progress` 가 보장된다.
    ///
    /// **왜 이것을 따로 고정하나.** fresh-start 정책은 마이그레이션 체인이 없다는 뜻이고,
    /// 그래서 v1 이 나간 뒤에 생긴 테이블은 `current == SCHEMA_VERSION` 갈래의 additive
    /// ensure 로만 기존 DB 에 닿는다. 그 갈래는 버전을 안 올리므로 스키마 변경을 여기에
    /// 얹어도 버전 값으로는 아무 신호가 안 난다 — 실측하면 그 줄을 지워도 나머지 시험이
    /// 전부 초록이었다. 없으면 기존 사용자의 튜토리얼 진행이 런타임 warn 으로만 깨진다.
    #[test]
    fn an_existing_database_still_gets_the_tutorial_table() {
        let mut conn = Connection::open_in_memory().unwrap();
        // v1 이 나갈 때의 DB — 본 스키마만 있고 뒤에 생긴 테이블은 없다.
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
