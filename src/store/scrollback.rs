//! `~/.tasty/scrollback/<persist_id>.bin` 디스크 영속 저장소.
//!
//! 레이아웃 슬롯(`~/.tasty/layouts/NN.json`)의
//! `SavedSurface::Terminal { scrollback_ref }` 가 가리키는
//! 파일을 관리한다. 직렬화 포맷은 `tasty_terminal::disk_scrollback::serialize_lines`
//! 와 동일 (magic + version + line records). lifecycle 은 host 책임:
//!
//! - capture: `restore_surface_content` 옵션 on 일 때 `write` 호출
//! - restore: 파일이 존재하면 `read` 후 inject
//! - surface close: `delete`
//! - 앱 시작 **1 회**: `gc_orphans(known_ids)` — `known_ids` 는 **전 슬롯**
//!   `scrollback_ref` 의 합집합이다(`core::layout_persistence::migrate_and_gc_on_boot`).
//!   슬롯 하나만 보고 정리하면 다른 슬롯이 참조하는 파일을 지운다. 읽을 수 없는
//!   슬롯이 하나라도 있으면 그 부팅에서는 정리를 **전면 스킵**한다 — 무엇을
//!   참조하는지 모르는 채 지우면 손상은 JSON 하나인데 손실은 scrollback 전체가 된다
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

/// scrollback 서브디렉. debug/release 격리는 루트(`tasty_home()`)가 담당하므로
/// 서브디렉 접미사(`-debug`)는 두지 않는다 — debug 는 `~/.tasty-debug/scrollback/`,
/// release 는 `~/.tasty/scrollback/`.
const SUBDIR: &str = "scrollback";
const EXT: &str = "bin";

/// Return `~/.tasty/scrollback/` (debug: `~/.tasty-debug/scrollback/`).
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

/// `known` 에 없는 `<dir>/*.bin` 을 지운다. `pub(crate)` 인 이유: layout 슬롯
/// union GC(`core::layout_persistence`)가 tempdir 로 단위 테스트되려면 디렉터리를
/// 직접 받는 진입점이 필요하다 — process-global `scrollback_dir()` 을 쓰는
/// [`gc_orphans`] 만으로는 테스트가 실제 홈을 건드린다.
pub(crate) fn gc_orphans_in(dir: &Path, known: &HashSet<String>) {
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

    /// 닫은 항목 복원 payload 의 전 구간: 살아 있는 터미널 → 캡처
    /// (`ClosedSurface`) → `persist_closed_scrollback` → 디스크 → `read_in`.
    ///
    /// 캡처가 저장 표현(`ScrollbackLine`)을 그대로 넘기도록 바뀌었다(이전에는
    /// `to_cells()` 로 cell 마다 `String` 을 재할당한 뒤 같은 표현으로 재압축).
    /// 그 변경이 복원 payload 를 바꾸지 않았음을 두 층위에서 고정한다:
    ///
    /// 1. **직렬화 바이트 동일** — 새 캡처 경로와 옛 재압축 경로가 만드는
    ///    디스크 바이트가 완전히 같다. 표현 변경이 저장물에 새는지를 직접 잡는다.
    /// 2. **왕복 보존** — 실제 write → read 후 라인 수 / 텍스트 / `wrapped` 가
    ///    원본과 같다.
    ///
    /// 주의: 디스크 포맷(`disk_scrollback::attr_flags`)이 싣는 속성은
    /// bold/half·italic·underline·strikethrough + fg/bg 뿐이라 `reverse` 등은
    /// 왕복에서 떨어진다. 이 테스트의 범위 밖(포맷 자체의 선존재 한계이며
    /// 고치려면 `FORMAT_VERSION` bump 가 필요하다) 이라 속성은 1번의 바이트
    /// 동일성으로만 확인하고, 2번에서는 포맷이 보장하는 것만 본다.
    #[test]
    fn capture_persist_restore_round_trip_preserves_lines() {
        use tasty_terminal::Terminal;

        let mut t = Terminal::new_detached(20, 4);
        t.set_scrollback_limit(100_000);
        for i in 0..40 {
            t.feed_bytes(format!("plain{i:03}\r\n").as_bytes());
            t.feed_bytes(b"\x1b[1;31mbold-red\x1b[0m tail\r\n");
            // 20 컬럼을 넘겨 auto-wrap 을 유발한다 → wrapped=true 라인이 생긴다.
            t.feed_bytes(b"0123456789012345678901234567890123456789\r\n");
            t.feed_bytes("\x1b[7m한글한글한글\x1b[0m\r\n".as_bytes());
        }
        let total = t.scrollback_len();
        assert!(total > 100, "스크롤백이 충분히 쌓여야 한다 (len={total})");

        // 캡처 (새 경로) — close 가 실제로 타는 함수.
        let mut item = crate::model::ClosedItem::Surface {
            surface: crate::model::closed_item::ClosedSurface::from_surface_id(1, Some(&t)),
            tab_name: "round-trip".to_string(),
        };

        // 옛 경로 재현: cell 로 풀었다가 같은 표현으로 재압축.
        let legacy: Vec<ScrollbackLine> = (0..total)
            .map(|i| {
                ScrollbackLine::new(
                    t.scrollback_line_owned(i).unwrap_or_default(),
                    t.scrollback_line_wrapped(i).unwrap_or(false),
                )
            })
            .collect();

        let dir = tempfile::tempdir().expect("tempdir");
        let mut captured = Vec::new();
        let mut persisted_id = None;
        crate::model::closed_item::persist_closed_scrollback(&mut item, &mut |lines| {
            captured = lines.to_vec();
            let id = new_persist_id();
            write_in(dir.path(), &id, lines).expect("write");
            persisted_id = Some(id.clone());
            Some(id)
        });
        let id = persisted_id.expect("스크롤백이 디스크로 영속화되어야 한다");

        // 1. 표현 변경이 저장 바이트를 바꾸지 않았다.
        assert_eq!(
            serialize_lines(&captured),
            serialize_lines(&legacy),
            "새 캡처 경로의 직렬화 결과가 옛 재압축 경로와 다르다"
        );

        // 2. 실제 왕복에서 라인 수 / 텍스트 / wrapped 가 보존된다.
        let got = read_in(dir.path(), &id).expect("read");
        assert_eq!(got.len(), total, "복원 라인 수가 원본과 다르다");
        for (i, line) in got.iter().enumerate() {
            let want_cells = t.scrollback_line_owned(i).unwrap_or_default();
            let want_text: String = want_cells.iter().map(|(g, _)| g.as_str()).collect();
            let got_text: String = line.to_cells().iter().map(|(g, _)| g.clone()).collect();
            assert_eq!(got_text, want_text, "line {i} 의 텍스트가 다르다");
            assert_eq!(
                line.wrapped,
                t.scrollback_line_wrapped(i).unwrap_or(false),
                "line {i} 의 wrapped 가 다르다"
            );
        }
        assert!(
            (0..total).any(|i| t.scrollback_line_wrapped(i).unwrap_or(false)),
            "wrapped 라인이 없으면 wrap 보존이 검증되지 않는다"
        );
        assert!(
            captured
                .iter()
                .flat_map(|l| l.to_cells())
                .any(|(_, a)| a != CellAttributes::default()),
            "비-default 속성이 없으면 1번의 바이트 동일성이 속성을 못 본다"
        );
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
