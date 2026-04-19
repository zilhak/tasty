//! `PRAGMA user_version` 기반의 단순 마이그레이션 체인.
//!
//! 새 버전 추가 절차:
//! 1. `MIGRATIONS` 배열 끝에 `(new_version, sql)` 튜플 추가.
//! 2. new_version은 앞 버전 + 1.
//! 3. SQL은 트랜잭션 안에서 실행된다. 중간 실패 시 롤백.

use anyhow::{Context, Result};
use rusqlite::Connection;

const MIGRATIONS: &[(u32, &str)] = &[
    (
        1,
        r#"
        CREATE TABLE IF NOT EXISTS meta (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS bookmarks (
            path TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            created_at INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS recent_markdown (
            path TEXT PRIMARY KEY,
            opened_at INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS recent_html (
            url TEXT PRIMARY KEY,
            opened_at INTEGER NOT NULL
        );

        -- 클립보드 히스토리 테이블(스키마 자리만 확보).
        -- 실제 write 연결은 후속 작업에서.
        CREATE TABLE IF NOT EXISTS clipboard_history (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            kind TEXT NOT NULL,     -- 'text' | 'image'
            text TEXT,              -- kind='text'일 때 값
            data BLOB,              -- kind='image'일 때 값
            source TEXT NOT NULL,   -- 'system' | 'internal'
            created_at INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_clipboard_history_created_at
            ON clipboard_history(created_at DESC);
        "#,
    ),
];

pub fn run(conn: &mut Connection) -> Result<()> {
    let current: u32 = conn
        .query_row("SELECT COALESCE(MIN(user_version), 0) FROM pragma_user_version()", [], |r| r.get(0))
        .unwrap_or(0);

    for (version, sql) in MIGRATIONS {
        if *version <= current {
            continue;
        }
        let tx = conn.transaction().context("begin migration tx")?;
        tx.execute_batch(sql)
            .with_context(|| format!("apply migration v{version}"))?;
        // PRAGMA user_version는 트랜잭션 내부에서도 설정 가능.
        tx.pragma_update(None, "user_version", version)
            .with_context(|| format!("set user_version to {version}"))?;
        tx.commit()
            .with_context(|| format!("commit migration v{version}"))?;
        tracing::info!("state.db migrated to v{version}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_create_expected_tables() {
        let mut conn = Connection::open_in_memory().unwrap();
        run(&mut conn).unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN ('meta','bookmarks','recent_markdown','recent_html','clipboard_history')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 5);

        // user_version is set.
        let ver: u32 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(ver, 1);
    }

    #[test]
    fn migrations_are_idempotent() {
        let mut conn = Connection::open_in_memory().unwrap();
        run(&mut conn).unwrap();
        // 두 번 호출해도 에러 없이 끝난다.
        run(&mut conn).unwrap();
    }
}
