//! 최근 연 Markdown / HTML 저장소.
//!
//! 저장: `~/.tasty/state.db` (SQLite) — `recent_markdown` / `recent_html` 테이블.
//! 인메모리 캐시(`AppState.recent_files`)가 매 뮤테이션마다 DB에 반영된다.
//!
//! `migrate_from_json()`은 앱 시작 시 `recent_files.json` → DB 1회 import.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// 파일 유형별 최대 보관 개수.
const MAX_ENTRIES: usize = 10;

#[derive(serde::Serialize, serde::Deserialize, Default, Clone)]
pub struct RecentFiles {
    pub markdown: Vec<String>,
    pub html: Vec<String>,
}

fn json_path() -> Option<PathBuf> {
    directories::BaseDirs::new()
        .map(|dirs| dirs.home_dir().join(".tasty").join("recent_files.json"))
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
        crate::storage::with_db(|db| {
            let markdown = query_list(&db.conn, "SELECT path FROM recent_markdown ORDER BY opened_at DESC LIMIT ?1");
            let html = query_list(&db.conn, "SELECT url FROM recent_html ORDER BY opened_at DESC LIMIT ?1");
            Self { markdown, html }
        })
        .unwrap_or_default()
    }

    pub fn add_markdown(&mut self, path: String) {
        self.markdown.retain(|p| p != &path);
        self.markdown.insert(0, path.clone());
        self.markdown.truncate(MAX_ENTRIES);
        let ts = now_secs();
        let _ = crate::storage::with_db(|db| upsert_markdown(&db.conn, &path, ts));
        prune_table("recent_markdown", "path");
    }

    pub fn add_html(&mut self, url: String) {
        self.html.retain(|u| u != &url);
        self.html.insert(0, url.clone());
        self.html.truncate(MAX_ENTRIES);
        let ts = now_secs();
        let _ = crate::storage::with_db(|db| upsert_html(&db.conn, &url, ts));
        prune_table("recent_html", "url");
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
    let rows = stmt.query_map(rusqlite::params![MAX_ENTRIES as i64], |r| r.get::<_, String>(0));
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

fn upsert_html(conn: &rusqlite::Connection, url: &str, ts: i64) {
    if let Err(e) = conn.execute(
        "INSERT INTO recent_html(url, opened_at) VALUES(?1, ?2)
         ON CONFLICT(url) DO UPDATE SET opened_at=excluded.opened_at",
        rusqlite::params![url, ts],
    ) {
        tracing::warn!("recent_html upsert failed: {e}");
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
    let _ = crate::storage::with_db(|db| {
        if let Err(e) = db.conn.execute(&sql, rusqlite::params![MAX_ENTRIES as i64]) {
            tracing::warn!("prune {table} failed: {e}");
        }
    });
}

/// 구 `recent_files.json`이 있고 DB에 아직 반영되지 않았다면 import 후
/// `recent_files.json.bak`으로 rename.
pub fn migrate_from_json() {
    let Some(path) = json_path() else { return; };
    if !path.exists() {
        return;
    }
    let Some(already) = crate::storage::with_db(|db| {
        db.conn
            .query_row(
                "SELECT value FROM meta WHERE key='recent_files_json_migrated'",
                [],
                |r| r.get::<_, String>(0),
            )
            .ok()
    }) else {
        return;
    };
    if already.is_some() {
        let bak = path.with_extension("json.bak");
        let _ = std::fs::rename(&path, &bak);
        return;
    }

    let Ok(contents) = std::fs::read_to_string(&path) else {
        return;
    };
    let Ok(parsed): Result<RecentFiles, _> = serde_json::from_str(&contents) else {
        tracing::warn!("recent_files.json parse failed — leaving file untouched");
        return;
    };

    let imported = crate::storage::with_db(|db| {
        let tx = match db.conn.transaction() {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!("recent_files migrate tx failed: {e}");
                return (0usize, 0usize);
            }
        };
        let mut n_md = 0;
        // JSON 순서: [0]이 가장 최신. opened_at을 그 순서대로 감소시켜 기록.
        let base = now_secs();
        for (i, p) in parsed.markdown.iter().enumerate() {
            let ts = base - i as i64;
            if let Err(e) = tx.execute(
                "INSERT OR IGNORE INTO recent_markdown(path, opened_at) VALUES(?1, ?2)",
                rusqlite::params![p, ts],
            ) {
                tracing::warn!("recent_markdown import row failed: {e}");
            } else {
                n_md += 1;
            }
        }
        let mut n_html = 0;
        for (i, u) in parsed.html.iter().enumerate() {
            let ts = base - i as i64;
            if let Err(e) = tx.execute(
                "INSERT OR IGNORE INTO recent_html(url, opened_at) VALUES(?1, ?2)",
                rusqlite::params![u, ts],
            ) {
                tracing::warn!("recent_html import row failed: {e}");
            } else {
                n_html += 1;
            }
        }
        if let Err(e) = tx.execute(
            "INSERT OR REPLACE INTO meta(key, value) VALUES ('recent_files_json_migrated', '1')",
            [],
        ) {
            tracing::warn!("recent_files migrate flag failed: {e}");
        }
        if let Err(e) = tx.commit() {
            tracing::warn!("recent_files migrate commit failed: {e}");
            return (0, 0);
        }
        (n_md, n_html)
    })
    .unwrap_or((0, 0));

    tracing::info!(
        "recent_files.json → state.db: md={} html={}",
        imported.0,
        imported.1
    );

    let bak = path.with_extension("json.bak");
    if let Err(e) = std::fs::rename(&path, &bak) {
        tracing::warn!("failed to rename recent_files.json to .bak: {e}");
    }
}
