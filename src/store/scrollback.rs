//! `~/.tasty/scrollback/<persist_id>.bin` 디스크 영속 저장소.
//!
//! `layout.json` 의 `SavedSurface::Terminal { scrollback_ref }` 가 가리키는
//! 파일을 관리한다. 직렬화 포맷은 `tasty_terminal::disk_scrollback::serialize_lines`
//! 와 동일 (magic + version + line records). lifecycle 은 host 책임:
//!
//! - capture: `restore_terminal_content` 옵션 on 일 때 `write` 호출
//! - restore: 파일이 존재하면 `read` 후 inject
//! - surface close: `delete`
//! - 앱 시작: `gc_orphans(known_ids)` 로 layout.json 에 없는 파일 정리
//! - 옵션 ON → OFF 전환: `clear_all()`
//!
//! Public API 는 `~/.tasty/scrollback/` 디렉터리를 사용한다. 내부 helper
//! (`*_in`) 는 임의 경로를 받아 단위 테스트가 process-global HOME 을
//! 건드리지 않게 한다.

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use tasty_terminal::ScrollbackLine;
use tasty_terminal::disk_scrollback::{deserialize_lines, serialize_lines};

/// Release: `scrollback`, Debug: `scrollback-debug`. debug 빌드를 release 와
/// 격리한다 — port file (`tasty-debug.port`) / layout (`layout-debug.json`) 과
/// 동일한 패턴.
const SUBDIR: &str = if cfg!(debug_assertions) {
    "scrollback-debug"
} else {
    "scrollback"
};
const EXT: &str = "bin";

/// Return `~/.tasty/scrollback/` (debug: `~/.tasty/scrollback-debug/`).
/// `None` 이면 home 디렉터리를 알 수 없음.
pub fn scrollback_dir() -> Option<PathBuf> {
    tasty_utils::path::tasty_home().map(|h| h.join(SUBDIR))
}

fn file_path_in(dir: &Path, persist_id: &str) -> Option<PathBuf> {
    if persist_id.is_empty() || persist_id.contains(['/', '\\', '.']) {
        return None;
    }
    Some(dir.join(format!("{persist_id}.{EXT}")))
}

/// Write `lines` atomically to `<dir>/<persist_id>.bin`.
fn write_in(dir: &Path, persist_id: &str, lines: &[ScrollbackLine]) -> io::Result<()> {
    let path = file_path_in(dir, persist_id)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid persist_id"))?;
    fs::create_dir_all(dir)?;
    let bytes = serialize_lines(lines);
    let tmp = path.with_extension(format!("{EXT}.tmp"));
    fs::write(&tmp, &bytes)?;
    fs::rename(&tmp, &path)
}

fn read_in(dir: &Path, persist_id: &str) -> Option<Vec<ScrollbackLine>> {
    let path = file_path_in(dir, persist_id)?;
    let bytes = fs::read(&path).ok()?;
    deserialize_lines(&bytes)
}

fn delete_in(dir: &Path, persist_id: &str) {
    let Some(path) = file_path_in(dir, persist_id) else {
        return;
    };
    match fs::remove_file(&path) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => tracing::warn!("scrollback_store: delete {} failed: {e}", path.display()),
    }
}

fn gc_orphans_in(dir: &Path, known: &HashSet<String>) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return,
        Err(e) => {
            tracing::warn!("scrollback_store: read_dir {} failed: {e}", dir.display());
            return;
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some(EXT) {
            continue;
        }
        let stem = match path.file_stem().and_then(|s| s.to_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };
        if !known.contains(&stem)
            && let Err(e) = fs::remove_file(&path)
        {
            tracing::warn!(
                "scrollback_store: orphan delete {} failed: {e}",
                path.display()
            );
        }
    }
}

// ── Public API (uses `~/.tasty/scrollback/`) ──

pub fn write(persist_id: &str, lines: &[ScrollbackLine]) -> io::Result<()> {
    let dir = scrollback_dir().ok_or_else(|| io::Error::other("cannot determine tasty home"))?;
    write_in(&dir, persist_id, lines)
}

pub fn read(persist_id: &str) -> Option<Vec<ScrollbackLine>> {
    let dir = scrollback_dir()?;
    read_in(&dir, persist_id)
}

pub fn delete(persist_id: &str) {
    let Some(dir) = scrollback_dir() else { return };
    delete_in(&dir, persist_id);
}

pub fn gc_orphans(known: &HashSet<String>) {
    let Some(dir) = scrollback_dir() else { return };
    gc_orphans_in(&dir, known);
}

pub fn clear_all() {
    gc_orphans(&HashSet::new());
}

/// Generate a fresh persist_id. 16-byte random value as lowercase hex.
pub fn new_persist_id() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 16];
    rand::rng().fill_bytes(&mut bytes);
    let mut s = String::with_capacity(32);
    for b in bytes {
        use std::fmt::Write;
        let _ = write!(s, "{b:02x}"); // String 의 fmt::Write 는 infallible — 항상 Ok, 무시.
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use termwiz::cell::CellAttributes;

    fn sample_lines() -> Vec<ScrollbackLine> {
        vec![
            ScrollbackLine::new(vec![("alpha".into(), CellAttributes::default())], false),
            ScrollbackLine::new(vec![("beta".into(), CellAttributes::default())], true),
        ]
    }

    #[test]
    fn write_then_read_round_trip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let id = new_persist_id();
        write_in(dir.path(), &id, &sample_lines()).expect("write");
        let out = read_in(dir.path(), &id).expect("read");
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].to_cells()[0].0, "alpha");
        assert!(!out[0].wrapped);
        assert_eq!(out[1].to_cells()[0].0, "beta");
        assert!(out[1].wrapped);
    }

    #[test]
    fn read_missing_returns_none() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(read_in(dir.path(), "nonexistent").is_none());
    }

    #[test]
    fn delete_is_idempotent() {
        let dir = tempfile::tempdir().expect("tempdir");
        let id = new_persist_id();
        write_in(dir.path(), &id, &sample_lines()).expect("write");
        delete_in(dir.path(), &id);
        delete_in(dir.path(), &id);
        assert!(read_in(dir.path(), &id).is_none());
    }

    #[test]
    fn gc_orphans_removes_unknown() {
        let dir = tempfile::tempdir().expect("tempdir");
        let keep = new_persist_id();
        let drop = new_persist_id();
        write_in(dir.path(), &keep, &sample_lines()).expect("write keep");
        write_in(dir.path(), &drop, &sample_lines()).expect("write drop");

        let mut known = HashSet::new();
        known.insert(keep.clone());
        gc_orphans_in(dir.path(), &known);

        assert!(read_in(dir.path(), &keep).is_some());
        assert!(read_in(dir.path(), &drop).is_none());
    }

    #[test]
    fn clear_all_removes_everything() {
        let dir = tempfile::tempdir().expect("tempdir");
        let a = new_persist_id();
        let b = new_persist_id();
        write_in(dir.path(), &a, &sample_lines()).expect("write a");
        write_in(dir.path(), &b, &sample_lines()).expect("write b");
        gc_orphans_in(dir.path(), &HashSet::new());
        assert!(read_in(dir.path(), &a).is_none());
        assert!(read_in(dir.path(), &b).is_none());
    }

    #[test]
    fn rejects_invalid_persist_ids() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(write_in(dir.path(), "", &sample_lines()).is_err());
        assert!(write_in(dir.path(), "../escape", &sample_lines()).is_err());
        assert!(write_in(dir.path(), "with.dot", &sample_lines()).is_err());
    }
}
