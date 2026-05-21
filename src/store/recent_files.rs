//! 최근 연 Markdown 저장소.
//!
//! 저장: `~/.tasty/state.db` (SQLite) — `recent_markdown` 테이블.
//! 인메모리 캐시(`AppState.recent_files`)가 매 뮤테이션마다 DB에 반영된다.

use std::time::{SystemTime, UNIX_EPOCH};

/// 파일 유형별 최대 보관 개수.
const MAX_ENTRIES: usize = 10;

#[derive(serde::Serialize, serde::Deserialize, Default, Clone)]
pub struct RecentFiles {
    pub markdown: Vec<String>,
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

impl RecentFiles {
    /// 앱 시작 시 1회 호출. 이후는 캐시를 사용.
    pub fn load() -> Self {
        crate::db::with_db(|db| {
            let markdown = query_list(
                &db.conn,
                "SELECT path FROM recent_markdown ORDER BY opened_at DESC LIMIT ?1",
            );
            Self { markdown }
        })
        .unwrap_or_default()
    }

    pub fn add_markdown(&mut self, path: String) {
        self.markdown.retain(|p| p != &path);
        self.markdown.insert(0, path.clone());
        self.markdown.truncate(MAX_ENTRIES);
        let ts = now_secs();
        if crate::db::with_db(|db| upsert_markdown(&db.conn, &path, ts)).is_none() {
            tracing::trace!("recent_files markdown upsert skipped: storage unavailable");
        }
        prune_table("recent_markdown", "path");
    }
}

fn query_list(conn: &rusqlite::Connection, sql: &str) -> Vec<String> {
    let mut stmt = match conn.prepare(sql) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("recent_files prepare failed: {e}");
            return Vec::new();
        }
    };
    let rows = stmt.query_map(rusqlite::params![MAX_ENTRIES as i64], |r| {
        r.get::<_, String>(0)
    });
    match rows {
        Ok(iter) => iter.filter_map(|r| r.ok()).collect(),
        Err(e) => {
            tracing::warn!("recent_files query failed: {e}");
            Vec::new()
        }
    }
}

fn upsert_markdown(conn: &rusqlite::Connection, path: &str, ts: i64) {
    if let Err(e) = conn.execute(
        "INSERT INTO recent_markdown(path, opened_at) VALUES(?1, ?2)
         ON CONFLICT(path) DO UPDATE SET opened_at=excluded.opened_at",
        rusqlite::params![path, ts],
    ) {
        tracing::warn!("recent_markdown upsert failed: {e}");
    }
}

/// 각 테이블에서 오래된 엔트리를 잘라 최신 MAX_ENTRIES개만 남긴다.
fn prune_table(table: &str, key_col: &str) {
    // SQLite에 동적 테이블명은 파라미터화할 수 없으므로 직접 포맷팅.
    // table / key_col은 소스 코드에서 고정된 값이라 SQL 인젝션 위험 없음.
    let sql = format!(
        "DELETE FROM {table} WHERE {key_col} NOT IN (
            SELECT {key_col} FROM {table} ORDER BY opened_at DESC LIMIT ?1
        )",
        table = table,
        key_col = key_col,
    );
    if crate::db::with_db(|db| {
        if let Err(e) = db.conn.execute(&sql, rusqlite::params![MAX_ENTRIES as i64]) {
            tracing::warn!("prune {table} failed: {e}");
        }
    })
    .is_none()
    {
        tracing::trace!("recent_files prune skipped: storage unavailable");
    }
}
