//! 최근 연 Markdown 저장소.
//!
//! 저장: `~/.tasty/state.db` (SQLite) — `recent_markdown` 테이블.
//! 인메모리 캐시(`AppState.recent_files`)가 매 뮤테이션마다 DB에 반영된다.
//!
//! **중복 정리**: 같은 파일을 가리키는 다른 경로 표기(구분자 `\`↔`/`, `\\?\`
//! verbatim prefix, `.`/`..` 세그먼트, Windows 대소문자 차)가 서로 다른 키로
//! 들어와 중복 행이 생기던 버그를 막는다. dedup 은 **정규화 키**(`dedup_key`)로
//! 비교하되 저장/표시/열기에는 **원본 raw path** 를 그대로 쓴다(과교정 방지).
//! DB 스키마는 fresh-start 정책(마이그레이션 체인 없음)이라, 기존 저장분의 중복은
//! `load()` 시 1회성 정리 패스로 접는다.

use std::collections::HashSet;
use std::path::Path;
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

/// dedup 비교 전용 정규화 키. 같은 파일을 가리키는 다른 표기를 하나로 접기 위한
/// 것으로 **표시·열기용이 아니다** — 원본 raw path 는 그대로 보존한다.
///
/// - `strip_verbatim_prefix`: `\\?\` extended-length prefix 제거.
/// - `lexically_normalize`: `.`/`..` 붕괴 + (Windows) 구분자 `/`→`\` 통일.
/// - Windows: 파일시스템이 대소문자 무시라 대소문자만 다른 경로는 같은 파일 → case fold.
fn dedup_key(path: &str) -> String {
    let stripped = tasty_utils::path::strip_verbatim_prefix(path);
    let normalized = tasty_utils::path::lexically_normalize(Path::new(&stripped));
    let key = normalized.to_string_lossy();
    #[cfg(windows)]
    {
        key.to_lowercase()
    }
    #[cfg(not(windows))]
    {
        key.into_owned()
    }
}

/// opened_at 내림차순으로 정렬된 `(path, opened_at)` 행에서 `dedup_key` 가 같은
/// 항목을 접는다. 첫 등장(=최신)만 `kept` 로, 나머지 raw path 는 `stale`(삭제 대상)
/// 로 분류. `kept` 는 입력 순서(최신순)를 유지한다. **순수 함수** — DB 접근 없음.
fn dedup_rows(rows: Vec<(String, i64)>) -> (Vec<String>, Vec<String>) {
    let mut seen: HashSet<String> = HashSet::new();
    let mut kept = Vec::new();
    let mut stale = Vec::new();
    for (path, _ts) in rows {
        if seen.insert(dedup_key(&path)) {
            kept.push(path);
        } else {
            stale.push(path);
        }
    }
    (kept, stale)
}

impl RecentFiles {
    /// 앱 시작 시 1회 호출. 이후는 캐시를 사용.
    ///
    /// 기존 저장분의 중복(구분자/대소문자/verbatim 차로 갈라진 행)을 정규화 키 기준
    /// 으로 접어(최신 opened_at 만 남기고 나머지 행 DELETE) 로드한다.
    pub fn load() -> Self {
        crate::db::with_db(|db| {
            let rows = query_rows(
                &db.conn,
                "SELECT path, opened_at FROM recent_markdown ORDER BY opened_at DESC",
            );
            let (mut kept, stale) = dedup_rows(rows);
            delete_paths(&db.conn, &stale);
            kept.truncate(MAX_ENTRIES);
            Self { markdown: kept }
        })
        .unwrap_or_default()
    }

    pub fn add_markdown(&mut self, path: String) {
        let key = dedup_key(&path);
        // 인메모리: 같은 정규화 키를 가진 옛 표기를 제거하고 최신 raw path 를 앞에.
        self.markdown.retain(|p| dedup_key(p) != key);
        self.markdown.insert(0, path.clone());
        self.markdown.truncate(MAX_ENTRIES);
        let ts = now_secs();
        if crate::db::with_db(|db| {
            // 같은 정규화 키의 기존 행(다른 raw 표기)을 제거한 뒤 upsert — DB 에도
            // dedup 을 반영해 중복 행이 물리적으로 남지 않게 한다.
            purge_same_key(&db.conn, &key, &path);
            upsert_markdown(&db.conn, &path, ts);
        })
        .is_none()
        {
            tracing::trace!("recent_files markdown upsert skipped: storage unavailable");
        }
        prune_table("recent_markdown", "path");
    }
}

fn query_rows(conn: &rusqlite::Connection, sql: &str) -> Vec<(String, i64)> {
    let mut stmt = match conn.prepare(sql) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("recent_files prepare failed: {e}");
            return Vec::new();
        }
    };
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)));
    match rows {
        Ok(iter) => iter.filter_map(|r| r.ok()).collect(),
        Err(e) => {
            tracing::warn!("recent_files query failed: {e}");
            Vec::new()
        }
    }
}

/// `paths` 의 행을 `recent_markdown` 에서 삭제. 빈 목록이면 no-op.
fn delete_paths(conn: &rusqlite::Connection, paths: &[String]) {
    for path in paths {
        if let Err(e) = conn.execute(
            "DELETE FROM recent_markdown WHERE path = ?1",
            rusqlite::params![path],
        ) {
            tracing::warn!("recent_markdown delete failed: {e}");
        }
    }
}

/// `key` 와 정규화 키가 같지만 raw path 는 `keep_path` 와 다른 기존 행을 삭제한다.
/// (테이블 상한 MAX_ENTRIES 라 전체 스캔 비용 무시 가능.)
fn purge_same_key(conn: &rusqlite::Connection, key: &str, keep_path: &str) {
    let existing = query_rows(conn, "SELECT path, opened_at FROM recent_markdown");
    let stale: Vec<String> = existing
        .into_iter()
        .map(|(p, _)| p)
        .filter(|p| p != keep_path && dedup_key(p) == key)
        .collect();
    delete_paths(conn, &stale);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(windows)]
    fn dedup_key_folds_separator_and_case_windows() {
        // 구분자 `\`↔`/` + 대소문자 차이는 같은 키로 접힌다.
        assert_eq!(dedup_key(r"E:\a\B.md"), dedup_key("E:/a/b.md"),);
        // verbatim `\\?\` prefix 유무도 같은 키.
        assert_eq!(dedup_key(r"\\?\E:\a\b.md"), dedup_key(r"E:\a\b.md"),);
        // `.`/`..` 세그먼트 붕괴.
        assert_eq!(dedup_key(r"E:\a\md\..\b.md"), dedup_key(r"E:\a\b.md"),);
    }

    #[test]
    #[cfg(windows)]
    fn dedup_key_keeps_distinct_files_windows() {
        assert_ne!(dedup_key(r"E:\a\b.md"), dedup_key(r"E:\a\c.md"));
    }

    #[test]
    #[cfg(not(windows))]
    fn dedup_key_folds_normalization_unix() {
        // Unix: 구분자/verbatim 은 no-op, `.`/`..` 만 붕괴. 대소문자는 구분 유지.
        assert_eq!(dedup_key("/a/md/../b.md"), dedup_key("/a/b.md"));
        assert_eq!(dedup_key("/a/./b.md"), dedup_key("/a/b.md"));
        // Unix 파일시스템은 대소문자 구분 → 다른 파일.
        assert_ne!(dedup_key("/a/B.md"), dedup_key("/a/b.md"));
    }

    #[test]
    fn dedup_rows_keeps_latest_per_key() {
        // 최신순(opened_at DESC) 입력. 같은 파일의 두 표기가 접혀야 한다.
        #[cfg(windows)]
        let (raw_new, raw_old, other) = (r"E:\a\b.md", "E:/a/B.md", r"E:\a\c.md");
        #[cfg(not(windows))]
        let (raw_new, raw_old, other) = ("/a/b.md", "/a/./b.md", "/a/c.md");

        let rows = vec![
            (raw_new.to_string(), 300),
            (other.to_string(), 200),
            (raw_old.to_string(), 100),
        ];
        let (kept, stale) = dedup_rows(rows);
        assert_eq!(kept, vec![raw_new.to_string(), other.to_string()]);
        assert_eq!(stale, vec![raw_old.to_string()]);
    }

    #[test]
    fn dedup_rows_noop_when_all_distinct() {
        #[cfg(windows)]
        let (a, b) = (r"E:\a.md", r"E:\b.md");
        #[cfg(not(windows))]
        let (a, b) = ("/a.md", "/b.md");
        let rows = vec![(a.to_string(), 2), (b.to_string(), 1)];
        let (kept, stale) = dedup_rows(rows);
        assert_eq!(kept, vec![a.to_string(), b.to_string()]);
        assert!(stale.is_empty());
    }
}
