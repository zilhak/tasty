//! 사용자가 picker 에서 직접 고른 handler 의 LRU 기록 (cap = 10).
//!
//! 저장: 플랫폼 공통 `~/.tasty/file-handler-recent.json` (project CLAUDE.md 의
//! `paths::tasty_home()` 사용). 원자적 쓰기는 temp + rename — fsync 는 안 함.
//!
//! 호출 흐름:
//! - 부팅 시 [`RecentPicks::load`] 로 디스크에서 1회 로드 → 인메모리 캐시.
//! - 사용자가 picker 에서 handler 선택 시 [`RecentPicks::record`] → 즉시
//!   [`RecentPicks::save_atomic`] 으로 디스크 반영.

use std::io;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::file::handler::HandlerId;

/// 기본 보관 개수.
pub const DEFAULT_CAP: usize = 10;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecentEntry {
    pub handler_id: HandlerId,
    /// unix epoch seconds.
    pub last_used_at: i64,
}

#[derive(Debug, Clone)]
pub struct RecentPicks {
    entries: Vec<RecentEntry>,
    cap: usize,
}

impl Default for RecentPicks {
    fn default() -> Self {
        Self::with_cap(DEFAULT_CAP)
    }
}

impl RecentPicks {
    pub fn with_cap(cap: usize) -> Self {
        Self {
            entries: Vec::new(),
            cap: cap.max(1),
        }
    }

    /// 파일에서 로드. 파일이 없거나 parse 실패 시 빈 리스트로 시작 + warn.
    pub fn load(path: &Path) -> Self {
        let text = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Self::default(),
            Err(e) => {
                tracing::warn!(
                    path = %path.display(),
                    error = %e,
                    "file_handler_recent: read failed",
                );
                return Self::default();
            }
        };
        match serde_json::from_str::<Persisted>(&text) {
            Ok(p) => {
                let mut me = Self::with_cap(DEFAULT_CAP);
                // 저장 파일 순서가 LRU 순서 (최근 → 오래된).
                for e in p.entries.into_iter().take(me.cap) {
                    me.entries.push(e);
                }
                me
            }
            Err(e) => {
                tracing::warn!(
                    path = %path.display(),
                    error = %e,
                    "file_handler_recent: parse failed — starting empty",
                );
                Self::default()
            }
        }
    }

    /// LRU 앞으로 옮긴다. 같은 id 가 있으면 dedupe. cap 초과 시 가장 오래된 entry drop.
    pub fn record(&mut self, id: &HandlerId) {
        self.record_at(id, now_secs());
    }

    /// 테스트용 — 명시적 timestamp.
    fn record_at(&mut self, id: &HandlerId, ts: i64) {
        self.entries.retain(|e| &e.handler_id != id);
        self.entries.insert(
            0,
            RecentEntry {
                handler_id: id.clone(),
                last_used_at: ts,
            },
        );
        self.entries.truncate(self.cap);
    }

    pub fn list(&self) -> &[RecentEntry] {
        &self.entries
    }

    /// 특정 handler id 를 LRU 에서 제거. 없으면 no-op (`false` 반환).
    pub fn forget(&mut self, id: &HandlerId) -> bool {
        let before = self.entries.len();
        self.entries.retain(|e| &e.handler_id != id);
        self.entries.len() != before
    }

    /// 원자적 쓰기 — `<path>.tmp` 작성 후 rename. fsync 는 안 함 (UX 영향).
    /// 부모 디렉토리는 호출자가 미리 만들어 둠 (없으면 그대로 에러).
    pub fn save_atomic(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let payload = Persisted {
            entries: self.entries.clone(),
        };
        let json = serde_json::to_string_pretty(&payload).map_err(io::Error::other)?;

        let tmp = tmp_path_for(path);
        std::fs::write(&tmp, json)?;
        match std::fs::rename(&tmp, path) {
            Ok(()) => Ok(()),
            Err(e) => {
                // rename 실패 시 임시 파일 정리 시도 — 결과는 무시 (정리만 함).
                let _ = std::fs::remove_file(&tmp);
                Err(e)
            }
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Persisted {
    entries: Vec<RecentEntry>,
}

fn tmp_path_for(path: &Path) -> std::path::PathBuf {
    let mut tmp = path.as_os_str().to_os_string();
    tmp.push(".tmp");
    std::path::PathBuf::from(tmp)
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hid(s: &str) -> HandlerId {
        HandlerId::new(s)
    }

    #[test]
    fn record_moves_to_front_and_dedupes() {
        let mut rp = RecentPicks::with_cap(3);
        rp.record_at(&hid("host/a"), 1);
        rp.record_at(&hid("host/b"), 2);
        rp.record_at(&hid("host/a"), 3);
        let ids: Vec<&str> = rp.list().iter().map(|e| e.handler_id.as_str()).collect();
        assert_eq!(ids, vec!["host/a", "host/b"]);
        assert_eq!(rp.list()[0].last_used_at, 3);
    }

    #[test]
    fn forget_removes_entry_returns_true() {
        let mut rp = RecentPicks::with_cap(3);
        rp.record_at(&hid("host/a"), 1);
        rp.record_at(&hid("host/b"), 2);
        assert!(rp.forget(&hid("host/a")));
        let ids: Vec<&str> = rp.list().iter().map(|e| e.handler_id.as_str()).collect();
        assert_eq!(ids, vec!["host/b"]);
    }

    #[test]
    fn forget_missing_id_returns_false_noop() {
        let mut rp = RecentPicks::with_cap(3);
        rp.record_at(&hid("host/a"), 1);
        assert!(!rp.forget(&hid("host/missing")));
        let ids: Vec<&str> = rp.list().iter().map(|e| e.handler_id.as_str()).collect();
        assert_eq!(ids, vec!["host/a"]);
    }

    #[test]
    fn cap_drops_oldest() {
        let mut rp = RecentPicks::with_cap(2);
        rp.record_at(&hid("host/a"), 1);
        rp.record_at(&hid("host/b"), 2);
        rp.record_at(&hid("host/c"), 3);
        let ids: Vec<&str> = rp.list().iter().map(|e| e.handler_id.as_str()).collect();
        assert_eq!(ids, vec!["host/c", "host/b"]);
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("recent.json");
        let mut rp = RecentPicks::with_cap(5);
        rp.record_at(&hid("host/markdown-viewer"), 100);
        rp.record_at(&hid("user/my-pdf"), 200);
        rp.save_atomic(&path).expect("save");

        let loaded = RecentPicks::load(&path);
        let ids: Vec<&str> = loaded
            .list()
            .iter()
            .map(|e| e.handler_id.as_str())
            .collect();
        assert_eq!(ids, vec!["user/my-pdf", "host/markdown-viewer"]);
    }

    #[test]
    fn load_missing_file_is_empty() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("does-not-exist.json");
        let rp = RecentPicks::load(&path);
        assert!(rp.list().is_empty());
    }

    #[test]
    fn load_garbage_warns_and_returns_empty() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("garbage.json");
        std::fs::write(&path, "this is not json").unwrap();
        let rp = RecentPicks::load(&path);
        assert!(rp.list().is_empty());
    }

    #[test]
    fn atomic_write_no_tmp_leftover() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("recent.json");
        let mut rp = RecentPicks::default();
        rp.record_at(&hid("host/x"), 1);
        rp.save_atomic(&path).expect("save");

        let tmp = tmp_path_for(&path);
        assert!(!tmp.exists(), "tmp file should be renamed away");
        assert!(path.exists());
    }
}
