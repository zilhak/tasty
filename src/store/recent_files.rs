//! 종류별 최근 파일 목록. 데이터 루트의 state.db에 저장하며 창들이 캐시를 공유한다.
//! 매니페스트의 records_recent를 사용하는 파일 열기 경로와 directory 같은 내장 종류가 기록한다.
//!
//! 중복 비교는 정규화한 경로로 하고 표시·열기에는 원래 경로를 사용한다.
//! 로드할 때 기존 중복 행을 정리하며, recent_markdown의 이전 데이터도 이관한다.
//! DB 쓰기에 실패해도 메모리 캐시 변경은 유지된다.

use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

/// 파일 유형별 최대 보관 개수.
const MAX_ENTRIES: usize = 10;

#[derive(Default, Clone)]
pub struct RecentFiles {
    /// 종류별 최신순 경로 목록.
    by_kind: Arc<Mutex<HashMap<String, Vec<String>>>>,
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// 중복 비교용 경로 키. 파일시스템의 동일 파일 여부를 확인하지는 않는다.
/// verbatim 접두사와 . / .. 표기를 정리하고 Windows에서는 소문자로 바꾼다.
/// 표시·열기에는 원래 경로를 사용한다.
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

/// 최신순 입력에서 같은 정규화 키의 첫 항목을 남기고 나머지를 삭제 대상으로 분류한다.
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
    /// 같은 DB의 창들이 캐시를 공유한다. 최초 로드에서 이전 테이블 이관과 중복 정리를 시도한다.
    pub fn load() -> Self {
        crate::db::with_state_db(Self::for_db).unwrap_or_default()
    }

    pub(crate) fn for_db(db: &mut crate::db::Db) -> Self {
        if let Some(recent) = &db.recent_files {
            return recent.clone();
        }
        let recent = Self::load_from_connection(&db.conn);
        db.recent_files = Some(recent.clone());
        recent
    }

    fn load_from_connection(conn: &rusqlite::Connection) -> Self {
        ensure_recent_files_table(conn);
        migrate_recent_markdown(conn);
        let rows = query_kind_rows(
            conn,
            "SELECT kind, path, opened_at FROM recent_files ORDER BY opened_at DESC",
        );
        let mut grouped: HashMap<String, Vec<(String, i64)>> = HashMap::new();
        for (kind, path, ts) in rows {
            grouped.entry(kind).or_default().push((path, ts));
        }
        let mut by_kind: HashMap<String, Vec<String>> = HashMap::new();
        for (kind, kind_rows) in grouped {
            let (mut kept, stale) = dedup_rows(kind_rows);
            delete_paths(conn, &kind, &stale);
            kept.truncate(MAX_ENTRIES);
            by_kind.insert(kind, kept);
        }
        Self {
            by_kind: Arc::new(Mutex::new(by_kind)),
        }
    }

    /// `kind` 의 최근 목록 스냅샷(최신순). 기록이 없으면 빈 배열.
    pub fn get(&self, kind: &str) -> Vec<String> {
        self.lock().get(kind).cloned().unwrap_or_default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Vec<String>>> {
        static POISONED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        crate::poison::recover_mutex(self.by_kind.lock(), "recent files cache", &POISONED)
    }

    /// 종류별 최신 목록에 추가하고 중복·개수 상한을 정리한다.
    pub fn add(&mut self, kind: &str, path: String) {
        let key = dedup_key(&path);
        // 캐시 갱신과 DB 저장 순서가 뒤바뀌지 않도록 같은 락 안에서 처리한다.
        let mut by_kind = self.lock();
        let list = by_kind.entry(kind.to_string()).or_default();
        list.retain(|p| dedup_key(p) != key);
        list.insert(0, path.clone());
        list.truncate(MAX_ENTRIES);
        let ts = now_secs();
        if crate::db::with_state_db(|db| {
            ensure_recent_files_table(&db.conn);
            purge_same_key(&db.conn, kind, &key, &path);
            upsert_recent(&db.conn, kind, &path, ts);
        })
        .is_none()
        {
            tracing::trace!("recent_files upsert skipped: storage unavailable");
        }
        prune_kind(kind);
    }
}

/// 이전 DB에도 테이블이 있도록 생성한다. 실패하면 경고를 남긴다.
fn ensure_recent_files_table(conn: &rusqlite::Connection) {
    if let Err(e) = conn.execute(
        "CREATE TABLE IF NOT EXISTS recent_files (
            kind TEXT NOT NULL,
            path TEXT NOT NULL,
            opened_at INTEGER NOT NULL,
            PRIMARY KEY(kind, path)
        )",
        [],
    ) {
        tracing::warn!("recent_files table ensure failed: {e}");
    }
}

/// recent_markdown을 이관하고 완료 플래그로 재실행을 막는다. 원래 테이블은 유지한다.
/// 복사나 플래그 기록에 실패하면 다음 로드에서 다시 시도할 수 있다.
fn migrate_recent_markdown(conn: &rusqlite::Connection) {
    let migrated: bool = conn
        .query_row(
            "SELECT 1 FROM meta WHERE key = 'recent_files_migrated'",
            [],
            |_| Ok(true),
        )
        .unwrap_or(false);
    if migrated {
        return;
    }
    let has_legacy: bool = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name='recent_markdown'",
            [],
            |_| Ok(true),
        )
        .unwrap_or(false);
    if has_legacy
        && let Err(e) = conn.execute(
            "INSERT OR IGNORE INTO recent_files(kind, path, opened_at)
             SELECT 'markdown', path, opened_at FROM recent_markdown",
            [],
        )
    {
        tracing::warn!("recent_markdown → recent_files migration failed: {e}");
        return;
    }
    if let Err(e) = conn.execute(
        "INSERT OR IGNORE INTO meta(key, value) VALUES('recent_files_migrated', '1')",
        [],
    ) {
        tracing::warn!("recent_files migration flag set failed: {e}");
    }
}

fn query_kind_rows(conn: &rusqlite::Connection, sql: &str) -> Vec<(String, String, i64)> {
    let mut stmt = match conn.prepare(sql) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("recent_files prepare failed: {e}");
            return Vec::new();
        }
    };
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, i64>(2)?,
        ))
    });
    match rows {
        Ok(iter) => iter.filter_map(|r| r.ok()).collect(),
        Err(e) => {
            tracing::warn!("recent_files query failed: {e}");
            Vec::new()
        }
    }
}

/// `paths` 의 행을 `recent_files` 에서 `kind` 별로 삭제. 빈 목록이면 no-op.
fn delete_paths(conn: &rusqlite::Connection, kind: &str, paths: &[String]) {
    for path in paths {
        if let Err(e) = conn.execute(
            "DELETE FROM recent_files WHERE kind = ?1 AND path = ?2",
            rusqlite::params![kind, path],
        ) {
            tracing::warn!("recent_files delete failed: {e}");
        }
    }
}

/// 정규화 키가 같고 원래 표기만 다른 경로를 종류별로 삭제한다.
fn purge_same_key(conn: &rusqlite::Connection, kind: &str, key: &str, keep_path: &str) {
    // query_kind_rows는 인자를 바인딩하지 않으므로 전체 조회 후 kind를 거른다.
    let existing = query_kind_rows(conn, "SELECT kind, path, opened_at FROM recent_files");
    let stale: Vec<String> = existing
        .into_iter()
        .filter(|(k, _, _)| k == kind)
        .map(|(_, p, _)| p)
        .filter(|p| p != keep_path && dedup_key(p) == key)
        .collect();
    delete_paths(conn, kind, &stale);
}

fn upsert_recent(conn: &rusqlite::Connection, kind: &str, path: &str, ts: i64) {
    if let Err(e) = conn.execute(
        "INSERT INTO recent_files(kind, path, opened_at) VALUES(?1, ?2, ?3)
         ON CONFLICT(kind, path) DO UPDATE SET opened_at=excluded.opened_at",
        rusqlite::params![kind, path, ts],
    ) {
        tracing::warn!("recent_files upsert failed: {e}");
    }
}

/// `kind` 안에서 오래된 엔트리를 잘라 최신 MAX_ENTRIES개만 남긴다.
fn prune_kind(kind: &str) {
    if crate::db::with_state_db(|db| {
        if let Err(e) = db.conn.execute(
            "DELETE FROM recent_files WHERE kind = ?1 AND path NOT IN (
                SELECT path FROM recent_files WHERE kind = ?1 ORDER BY opened_at DESC LIMIT ?2
            )",
            rusqlite::params![kind, MAX_ENTRIES as i64],
        ) {
            tracing::warn!("prune recent_files failed: {e}");
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
    fn windows_using_the_same_database_share_recent_updates() {
        let dir = tempfile::tempdir().unwrap();
        let mut db = crate::db::Db::open(&dir.path().join("state.db")).unwrap();
        let mut first = RecentFiles::for_db(&mut db);
        let mut second = RecentFiles::for_db(&mut db);
        assert!(first.get("markdown").is_empty());
        assert!(second.get("markdown").is_empty());
        first.add("markdown", "/notes/first.md".into());
        assert_eq!(second.get("markdown"), vec!["/notes/first.md"]);
        second.add("markdown", "/notes/second.md".into());
        first.add("markdown", "/notes/./first.md".into());
        assert_eq!(first.get("markdown"), second.get("markdown"));
        assert_eq!(second.get("markdown")[0], "/notes/./first.md");
        for i in 0..12 {
            second.add("markdown", format!("/notes/{i}.md"));
        }
        assert_eq!(first.get("markdown"), second.get("markdown"));
        assert_eq!(first.get("markdown").len(), MAX_ENTRIES);
        assert_eq!(first.get("markdown")[0], "/notes/11.md");
        let later = RecentFiles::for_db(&mut db);
        assert_eq!(later.get("markdown"), first.get("markdown"));
        assert!(later.get("directory").is_empty());
        let mut other_db = crate::db::Db::open(&dir.path().join("other.db")).unwrap();
        assert!(
            RecentFiles::for_db(&mut other_db)
                .get("markdown")
                .is_empty()
        );
    }

    #[test]
    #[cfg(windows)]
    fn dedup_key_folds_separator_and_case_windows() {
        assert_eq!(dedup_key(r"E:\a\B.md"), dedup_key("E:/a/b.md"),);
        assert_eq!(dedup_key(r"\\?\E:\a\b.md"), dedup_key(r"E:\a\b.md"),);
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
        assert_eq!(dedup_key("/a/md/../b.md"), dedup_key("/a/b.md"));
        assert_eq!(dedup_key("/a/./b.md"), dedup_key("/a/b.md"));
        assert_ne!(dedup_key("/a/B.md"), dedup_key("/a/b.md"));
    }

    #[test]
    fn dedup_rows_keeps_latest_per_key() {
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

    #[test]
    fn get_absent_kind_is_empty() {
        let rf = RecentFiles::default();
        assert!(rf.get("markdown").is_empty());
    }

    #[test]
    fn directory_kind_recent_latest_first_dedup_and_cap() {
        let mut rf = RecentFiles::default();
        #[cfg(windows)]
        let mk = |i: usize| format!(r"E:\dir{i}");
        #[cfg(not(windows))]
        let mk = |i: usize| format!("/dir{i}");

        for i in 0..12 {
            rf.add("directory", mk(i));
        }
        let list = rf.get("directory");
        assert_eq!(list.len(), MAX_ENTRIES);
        assert_eq!(list[0], mk(11)); // 마지막 add 가 맨 앞.
        assert_eq!(list[9], mk(2)); // 가장 오래된 유지분.

        rf.add("directory", mk(2));
        let list = rf.get("directory");
        assert_eq!(list.len(), MAX_ENTRIES);
        assert_eq!(list[0], mk(2));
        assert_eq!(list.iter().filter(|p| **p == mk(2)).count(), 1);

        assert!(rf.get("markdown").is_empty());
    }

    #[test]
    fn add_and_get_in_memory_dedup() {
        // DB가 없어도 캐시는 갱신돼야 한다.
        let mut rf = RecentFiles::default();
        #[cfg(windows)]
        let (p1, p1_alt, p2) = (r"E:\a\b.md", "E:/a/B.md", r"E:\a\c.md");
        #[cfg(not(windows))]
        let (p1, p1_alt, p2) = ("/a/b.md", "/a/./b.md", "/a/c.md");

        rf.add("markdown", p2.to_string());
        rf.add("markdown", p1.to_string());
        rf.add("markdown", p1_alt.to_string());

        let list = rf.get("markdown");
        assert_eq!(list.len(), 2);
        assert_eq!(list[0], p1_alt.to_string());
        assert_eq!(list[1], p2.to_string());
        assert!(rf.get("html").is_empty());
    }
}
