//! Explorer 북마크 저장소.
//!
//! 저장: `~/.tasty/state.db` (SQLite) — `bookmarks` 테이블.
//! 구 `bookmarks.json`은 앱 시작 시 `migrate_from_json()`으로 1회 import 되고
//! `bookmarks.json.bak`으로 rename 된다.
//!
//! 스키마:
//! ```sql
//! CREATE TABLE bookmarks (
//!     path TEXT PRIMARY KEY,
//!     name TEXT NOT NULL,
//!     created_at INTEGER NOT NULL  -- unix seconds
//! );
//! ```
//!
//! 외부 API는 파일 기반이었던 기존 구조와 호환을 유지한다.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct BookmarkEntry {
    pub name: String,
    pub path: String,
}

#[derive(serde::Serialize, serde::Deserialize, Default)]
pub struct Bookmarks {
    pub entries: Vec<BookmarkEntry>,
}

fn json_path() -> Option<PathBuf> {
    directories::BaseDirs::new()
        .map(|dirs| dirs.home_dir().join(".tasty").join("bookmarks.json"))
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

impl Bookmarks {
    /// DB에서 모든 엔트리를 가장 최근 추가 순(내림차순)으로 읽어온다.
    pub fn load() -> Self {
        let entries = crate::storage::with_db(|db| {
            let mut stmt = match db.conn.prepare(
                "SELECT name, path FROM bookmarks ORDER BY created_at DESC, path ASC",
            ) {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!("bookmarks load prepare failed: {e}");
                    return Vec::new();
                }
            };
            let iter = match stmt.query_map([], |row| {
                Ok(BookmarkEntry {
                    name: row.get(0)?,
                    path: row.get(1)?,
                })
            }) {
                Ok(i) => i,
                Err(e) => {
                    tracing::warn!("bookmarks query failed: {e}");
                    return Vec::new();
                }
            };
            iter.filter_map(|r| r.ok()).collect()
        })
        .unwrap_or_default();
        Self { entries }
    }

    /// 경로 기준으로 upsert. 이미 있으면 이름과 created_at 갱신.
    pub fn add(&mut self, name: String, path: String) {
        // 인메모리 cache 갱신(기존 동작 유지: 같은 경로 덮어쓰기 후 push).
        self.entries.retain(|b| b.path != path);
        self.entries.push(BookmarkEntry {
            name: name.clone(),
            path: path.clone(),
        });

        let ts = now_secs();
        let _ = crate::storage::with_db(|db| {
            if let Err(e) = db.conn.execute(
                "INSERT INTO bookmarks(path, name, created_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT(path) DO UPDATE SET name=excluded.name, created_at=excluded.created_at",
                rusqlite::params![path, name, ts],
            ) {
                tracing::warn!("bookmarks insert failed: {e}");
            }
        });
    }

    pub fn remove(&mut self, path: &str) {
        self.entries.retain(|b| b.path != path);
        let _ = crate::storage::with_db(|db| {
            if let Err(e) = db.conn.execute("DELETE FROM bookmarks WHERE path = ?1", rusqlite::params![path]) {
                tracing::warn!("bookmarks delete failed: {e}");
            }
        });
    }

    pub fn is_bookmarked(&self, path: &str) -> bool {
        self.entries.iter().any(|b| b.path == path)
    }
}

/// 구 `bookmarks.json`이 있고 DB에 아직 반영되지 않았다면 import 후
/// `bookmarks.json.bak`으로 rename. 이미 마이그레이션 완료 플래그가 있으면 no-op.
pub fn migrate_from_json() {
    let Some(path) = json_path() else { return; };
    if !path.exists() {
        return;
    }
    // DB 접근 불가면 포기 (인메모리 폴백 중 json을 소실시키면 안 됨).
    let Some(already) = crate::storage::with_db(|db| {
        db.conn
            .query_row(
                "SELECT value FROM meta WHERE key='bookmarks_json_migrated'",
                [],
                |r| r.get::<_, String>(0),
            )
            .ok()
    }) else {
        return;
    };
    if already.is_some() {
        // 이미 마이그레이션 됨 — 안전하게 json이 여전히 존재한다면 .bak로 이름 변경.
        let bak = path.with_extension("json.bak");
        let _ = std::fs::rename(&path, &bak);
        return;
    }

    let Ok(contents) = std::fs::read_to_string(&path) else {
        return;
    };
    let Ok(parsed): Result<Bookmarks, _> = serde_json::from_str(&contents) else {
        tracing::warn!("bookmarks.json parse failed — leaving file untouched");
        return;
    };

    let ts = now_secs();
    let imported = crate::storage::with_db(|db| {
        let tx = match db.conn.transaction() {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!("bookmarks migrate tx failed: {e}");
                return 0usize;
            }
        };
        let mut n = 0;
        for entry in &parsed.entries {
            // INSERT OR IGNORE: DB에 이미 같은 path가 있으면 json 값 사용 안 함.
            let res = tx.execute(
                "INSERT OR IGNORE INTO bookmarks(path, name, created_at) VALUES (?1, ?2, ?3)",
                rusqlite::params![entry.path, entry.name, ts],
            );
            match res {
                Ok(c) => n += c,
                Err(e) => tracing::warn!("bookmarks import row failed: {e}"),
            }
        }
        if let Err(e) = tx.execute(
            "INSERT OR REPLACE INTO meta(key, value) VALUES ('bookmarks_json_migrated', '1')",
            [],
        ) {
            tracing::warn!("bookmarks migrate flag failed: {e}");
        }
        if let Err(e) = tx.commit() {
            tracing::warn!("bookmarks migrate commit failed: {e}");
            return 0;
        }
        n
    })
    .unwrap_or(0);

    tracing::info!("bookmarks.json → state.db: {imported} entries imported");

    // DB 커밋 성공 후에만 원본 rename.
    let bak = path.with_extension("json.bak");
    if let Err(e) = std::fs::rename(&path, &bak) {
        tracing::warn!("failed to rename bookmarks.json to .bak: {e}");
    }
}
