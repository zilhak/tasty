//! 최근 연 파일 저장소 (generic per-kind).
//!
//! 저장: `~/.tasty/state.db` (SQLite) — `recent_files(kind, path, opened_at)` 테이블.
//! 인메모리 캐시(`AppState.recent_files`)가 매 뮤테이션마다 DB에 반영된다. host 는
//! 특정 surface_kind 이름을 모르고, 매니페스트 `records_recent` 를 선언한 kind 만
//! 파일-open 진입점에서 기록 대상이 된다(generic per-kind — kind 하드코딩 없음).
//! 그 외에 builtin host surface 가 자체 kind 로 직접 적재하기도 한다(예: explorer 가
//! 이동 확정한 cwd 를 `"directory"` kind 로 — `add`/`get` 은 kind 문자열만 다를 뿐 동일 경로).
//!
//! **레거시 마이그레이션**: 이전 버전의 `recent_markdown(path, opened_at)` 데이터는
//! 앱 시작 시 1회 `recent_files` 로 복사된다(`kind='markdown'`). meta 플래그로 정확히
//! 한 번만 복사해 pruned 엔트리의 부활을 막고, old 테이블은 남겨둔다(데이터 유실 금지).
//!
//! **중복 정리**: 같은 파일을 가리키는 다른 경로 표기(구분자 `\`↔`/`, `\\?\`
//! verbatim prefix, `.`/`..` 세그먼트, Windows 대소문자 차)가 서로 다른 키로
//! 들어와 중복 행이 생기던 버그를 막는다. dedup 은 **정규화 키**(`dedup_key`)로
//! 비교하되 저장/표시/열기에는 **원본 raw path** 를 그대로 쓴다(과교정 방지).
//! DB 스키마는 fresh-start 정책(마이그레이션 체인 없음)이라, 기존 저장분의 중복은
//! `load()` 시 1회성 정리 패스로 접는다.

use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// 파일 유형별 최대 보관 개수.
const MAX_ENTRIES: usize = 10;

#[derive(serde::Serialize, serde::Deserialize, Default, Clone)]
pub struct RecentFiles {
    /// surface_kind → 최신순 경로 목록. `records_recent` 를 선언한 kind 만 채워진다.
    pub by_kind: HashMap<String, Vec<String>>,
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
    /// `recent_files` 테이블을 보장(레거시 DB 대비)하고 `recent_markdown` 레거시
    /// 데이터를 1회 마이그레이션한 뒤, kind 별로 로드한다. 기존 저장분의 중복(구분자/
    /// 대소문자/verbatim 차로 갈라진 행)을 정규화 키 기준으로 접어(최신 opened_at 만
    /// 남기고 나머지 행 DELETE) 로드한다.
    pub fn load() -> Self {
        crate::db::with_db(|db| {
            ensure_recent_files_table(&db.conn);
            migrate_recent_markdown(&db.conn);
            let rows = query_kind_rows(
                &db.conn,
                "SELECT kind, path, opened_at FROM recent_files ORDER BY opened_at DESC",
            );
            let mut grouped: HashMap<String, Vec<(String, i64)>> = HashMap::new();
            for (kind, path, ts) in rows {
                grouped.entry(kind).or_default().push((path, ts));
            }
            let mut by_kind: HashMap<String, Vec<String>> = HashMap::new();
            for (kind, kind_rows) in grouped {
                let (mut kept, stale) = dedup_rows(kind_rows);
                delete_paths(&db.conn, &kind, &stale);
                kept.truncate(MAX_ENTRIES);
                by_kind.insert(kind, kept);
            }
            Self { by_kind }
        })
        .unwrap_or_default()
    }

    /// `kind` 의 최근 목록(최신순). 기록이 없으면 빈 슬라이스.
    pub fn get(&self, kind: &str) -> &[String] {
        self.by_kind.get(kind).map_or(&[], |v| v.as_slice())
    }

    /// `kind` 의 최근 목록에 `path` 를 최신으로 추가한다(정규화 dedup + 상한).
    pub fn add(&mut self, kind: &str, path: String) {
        let key = dedup_key(&path);
        // 인메모리: 같은 정규화 키를 가진 옛 표기를 제거하고 최신 raw path 를 앞에.
        let list = self.by_kind.entry(kind.to_string()).or_default();
        list.retain(|p| dedup_key(p) != key);
        list.insert(0, path.clone());
        list.truncate(MAX_ENTRIES);
        let ts = now_secs();
        if crate::db::with_db(|db| {
            ensure_recent_files_table(&db.conn);
            // 같은 정규화 키의 기존 행(다른 raw 표기)을 제거한 뒤 upsert — DB 에도
            // dedup 을 반영해 중복 행이 물리적으로 남지 않게 한다.
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

/// `recent_files` 테이블을 보장한다. fresh DB 는 schema 로 이미 생성되지만, 레거시
/// (user_version 이 이미 SCHEMA_VERSION 인) DB 는 이 테이블이 없을 수 있어 방어적으로
/// 만든다. `IF NOT EXISTS` 라 기존 데이터는 건드리지 않는다.
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

/// 레거시 `recent_markdown` 데이터를 `recent_files`(kind='markdown')로 **정확히 1회**
/// 복사한다. meta 플래그(`recent_files_migrated`)로 재실행을 막아, 사용자가 이후 pruned
/// 시킨 엔트리가 매 부팅마다 부활하는 것을 방지한다. old 테이블은 남겨둔다(데이터 유실
/// 금지). 복사 실패 시 플래그를 세우지 않아 다음 부팅에 재시도한다.
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

/// `key` 와 정규화 키가 같지만 raw path 는 `keep_path` 와 다른 기존 행을 `kind` 안에서
/// 삭제한다. (kind 별 상한 MAX_ENTRIES 라 전체 스캔 비용 무시 가능.)
fn purge_same_key(conn: &rusqlite::Connection, kind: &str, key: &str, keep_path: &str) {
    // `query_kind_rows` 는 파라미터를 0개 바인딩하므로 WHERE 절에 placeholder 를 두면
    // NULL 로 처리돼 매치가 0 이 된다. WHERE 없이 전체를 읽고 아래에서 Rust 로 kind 필터.
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
    if crate::db::with_db(|db| {
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

    #[test]
    fn get_absent_kind_is_empty() {
        let rf = RecentFiles::default();
        assert!(rf.get("markdown").is_empty());
    }

    /// explorer 최근 디렉토리(kind="directory")도 generic 경로를 그대로 탄다 —
    /// 최신순·중복제거·상한(MAX_ENTRIES)·kind 격리. explorer address_bar 후보 소스.
    #[test]
    fn directory_kind_recent_latest_first_dedup_and_cap() {
        let mut rf = RecentFiles::default();
        #[cfg(windows)]
        let mk = |i: usize| format!(r"E:\dir{i}");
        #[cfg(not(windows))]
        let mk = |i: usize| format!("/dir{i}");

        // 상한(10) 초과로 12개 적재 → 최신 10개만, 최신순.
        for i in 0..12 {
            rf.add("directory", mk(i));
        }
        let list = rf.get("directory");
        assert_eq!(list.len(), MAX_ENTRIES);
        assert_eq!(list[0], mk(11)); // 마지막 add 가 맨 앞.
        assert_eq!(list[9], mk(2)); // 가장 오래된 유지분.

        // 재방문 → 중복 없이 맨 앞으로.
        rf.add("directory", mk(2));
        let list = rf.get("directory");
        assert_eq!(list.len(), MAX_ENTRIES);
        assert_eq!(list[0], mk(2));
        assert_eq!(list.iter().filter(|p| **p == mk(2)).count(), 1);

        // kind 격리 — markdown 은 비어 있다.
        assert!(rf.get("markdown").is_empty());
    }

    #[test]
    fn add_and_get_in_memory_dedup() {
        // DB 가 없어도 인메모리 캐시 뮤테이션은 동작한다(add 는 DB 실패를 trace 로 흡수).
        let mut rf = RecentFiles::default();
        #[cfg(windows)]
        let (p1, p1_alt, p2) = (r"E:\a\b.md", "E:/a/B.md", r"E:\a\c.md");
        #[cfg(not(windows))]
        let (p1, p1_alt, p2) = ("/a/b.md", "/a/./b.md", "/a/c.md");

        rf.add("markdown", p2.to_string());
        rf.add("markdown", p1.to_string());
        // 같은 파일의 다른 표기 → 옛 표기 제거 후 최신 raw 를 앞에.
        rf.add("markdown", p1_alt.to_string());

        let list = rf.get("markdown");
        assert_eq!(list.len(), 2);
        assert_eq!(list[0], p1_alt.to_string());
        assert_eq!(list[1], p2.to_string());
        // kind 격리 — 다른 kind 는 빈 목록.
        assert!(rf.get("html").is_empty());
    }
}
