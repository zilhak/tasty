//! 데이터 루트의 scrollback/<persist_id>.bin 저장소.
//! 레이아웃 슬롯의 scrollback_ref가 파일을 가리키며 포맷은
//! tasty_terminal::disk_scrollback::serialize_lines를 따른다.
//!
//! Product journal import reads the selected home's original bytes before copying them into BLOBs.
//! Legacy file cleanup still uses slot references; immutable journal payloads use their own pins/GC.
//! Direct runtime capture and decoded legacy reads remain compatibility-test helpers.

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[cfg(test)]
use tasty_terminal::ScrollbackLine;
#[cfg(test)]
use tasty_terminal::disk_scrollback::deserialize_lines;

// 루트 경로 선택과 debug/release 구분은 tasty_home()에서 처리한다.
const SUBDIR: &str = "scrollback";
const EXT: &str = "bin";

/// 데이터 루트의 scrollback 디렉터리. 루트를 구하지 못하면 None이다.
pub fn scrollback_dir() -> Option<PathBuf> {
    tasty_utils::path::tasty_home().map(|h| h.join(SUBDIR))
}

fn file_path_in(dir: &Path, persist_id: &str) -> Option<PathBuf> {
    if persist_id.is_empty() || persist_id.contains(['/', '\\', '.']) {
        return None;
    }
    Some(dir.join(format!("{persist_id}.{EXT}")))
}

// Legacy capture fixtures write files; product snapshots are immutable journal payloads.
#[cfg(test)]
fn write_in(dir: &Path, persist_id: &str, lines: &[ScrollbackLine]) -> io::Result<()> {
    write_bytes_in(
        dir,
        persist_id,
        &tasty_terminal::disk_scrollback::serialize_lines(lines),
    )
}

/// 이미 `serialize_lines` 형식으로 인코딩한 바이트를 쓴다. 임시 파일에 쓴 뒤 rename으로
/// 대상 경로를 교체한다.
#[cfg(test)]
fn write_bytes_in(dir: &Path, persist_id: &str, bytes: &[u8]) -> io::Result<()> {
    let path = file_path_in(dir, persist_id)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid persist_id"))?;
    fs::create_dir_all(dir)?;
    let tmp = path.with_extension(format!("{EXT}.tmp"));
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, &path)
}

/// Read a legacy immutable capture from the explicitly selected data home during journal import.
pub(crate) fn read_bytes_from_home(home: &Path, persist_id: &str) -> io::Result<Option<Vec<u8>>> {
    let path = file_path_in(&home.join(SUBDIR), persist_id)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid persist_id"))?;
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

/// 파일 부재와 읽기 실패를 구분한다. 복원 실패 시 원본을 남겨 재시도할 수 있어야 한다.
#[cfg(test)]
pub enum ScrollbackRead {
    /// 정상적으로 읽고 역직렬화했다(빈 목록일 수 있다).
    Loaded(Vec<ScrollbackLine>),
    /// 경로를 찾지 못했다.
    Absent,
    /// 읽기·해석에 실패했거나 persist_id가 무효다. 원본은 지우지 않는다.
    Unreadable,
}

#[cfg(test)]
fn read_in(dir: &Path, persist_id: &str) -> ScrollbackRead {
    let Some(path) = file_path_in(dir, persist_id) else {
        tracing::warn!("scrollback read: invalid persist_id {persist_id:?}");
        return ScrollbackRead::Unreadable;
    };
    match fs::read(&path) {
        Ok(bytes) => decode_lines(&path, &bytes),
        Err(e) if e.kind() == io::ErrorKind::NotFound => ScrollbackRead::Absent,
        Err(e) => {
            tracing::warn!(
                "scrollback read: {} failed: {e} — restoring without it and keeping the file",
                path.display()
            );
            ScrollbackRead::Unreadable
        }
    }
}

#[cfg(test)]
fn decode_lines(path: &Path, bytes: &[u8]) -> ScrollbackRead {
    match deserialize_lines(bytes) {
        Some(lines) => ScrollbackRead::Loaded(lines),
        None => {
            tracing::warn!(
                "scrollback read: {} is corrupt — restoring without it and keeping the file",
                path.display()
            );
            ScrollbackRead::Unreadable
        }
    }
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

/// known에 없는 bin 파일을 삭제한다. 임시 디렉터리로 GC를 시험할 수 있도록 경로를 받는다.
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

#[cfg(test)]
pub fn write(persist_id: &str, lines: &[ScrollbackLine]) -> io::Result<()> {
    let dir = scrollback_dir().ok_or_else(|| io::Error::other("cannot determine tasty home"))?;
    write_in(&dir, persist_id, lines)
}

/// 닫은 항목의 인코딩된 스크롤백을 줄로 푼다. 형식이 맞지 않으면 None이다.
#[cfg(test)]
pub fn decode_blob(
    blob: &crate::model::closed_item::ScrollbackBlob,
) -> Option<Vec<ScrollbackLine>> {
    deserialize_lines(&blob.bytes)
}

#[cfg(test)]
pub fn read(persist_id: &str) -> ScrollbackRead {
    match scrollback_dir() {
        Some(dir) => read_in(&dir, persist_id),
        None => ScrollbackRead::Unreadable,
    }
}

pub fn delete(persist_id: &str) {
    let Some(dir) = scrollback_dir() else { return };
    delete_in(&dir, persist_id);
}

/// GUI 부팅의 GC 진입점. headless에서는 gc_orphans_in을 직접 사용한다.
#[cfg(feature = "gui")]
pub fn gc_orphans(known: &HashSet<String>) {
    let Some(dir) = scrollback_dir() else { return };
    gc_orphans_in(&dir, known);
}

/// 설정 화면의 "scrollback 전부 지우기". GUI 전용 입력이다.
#[cfg(feature = "gui")]
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
    use tasty_terminal::disk_scrollback::serialize_lines;
    use termwiz::cell::CellAttributes;

    fn expect_lines(dir: &Path, id: &str) -> Vec<ScrollbackLine> {
        match read_in(dir, id) {
            ScrollbackRead::Loaded(lines) => lines,
            ScrollbackRead::Absent => panic!("scrollback {id} 이 없다"),
            ScrollbackRead::Unreadable => panic!("scrollback {id} 을 읽지 못했다"),
        }
    }

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
        let out = expect_lines(dir.path(), &id);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].to_cells()[0].0, "alpha");
        assert!(!out[0].wrapped);
        assert_eq!(out[1].to_cells()[0].0, "beta");
        assert!(out[1].wrapped);
    }

    // 캡처·직렬화·디스크 왕복을 검사한다. 셀로 풀어 재압축한 결과와 바이트도 대조한다.
    // 디스크 포맷에 없는 reverse 같은 속성은 왕복에서 사라지므로,
    // 속성은 바이트 비교로 확인하고 왕복은 텍스트·행 수·wrapped를 비교한다.
    #[test]
    fn capture_persist_restore_round_trip_preserves_lines() {
        use tasty_terminal::Terminal;

        let mut t = Terminal::new_detached(20, 4);
        t.set_scrollback_limit(100_000);
        for i in 0..40 {
            t.feed_bytes(format!("plain{i:03}\r\n").as_bytes());
            t.feed_bytes(b"\x1b[1;31mbold-red\x1b[0m tail\r\n");
            t.feed_bytes(b"0123456789012345678901234567890123456789\r\n");
            t.feed_bytes("\x1b[7m한글한글한글\x1b[0m\r\n".as_bytes());
        }
        let total = t.scrollback_len();
        assert!(total > 100, "스크롤백이 충분히 쌓여야 한다 (len={total})");

        let mut item = crate::model::ClosedItem::Surface {
            surface: crate::model::closed_item::ClosedSurface::from_capture(
                1,
                Some(crate::runtime::terminal_store::closed_capture_of(
                    &t,
                    t.cached_cwd(),
                )),
            ),
            tab_name: "round-trip".to_string(),
        };

        // 비교를 위해 각 셀로 풀었다가 다시 압축한다.
        let legacy: Vec<ScrollbackLine> = (0..total)
            .map(|i| {
                ScrollbackLine::new(
                    t.scrollback_line_owned(i).unwrap_or_default(),
                    t.scrollback_line_wrapped(i).unwrap_or(false),
                )
            })
            .collect();

        let dir = tempfile::tempdir().expect("tempdir");
        let mut captured_blob = None;
        let mut persisted_id = None;
        crate::model::closed_item::persist_closed_scrollback(&mut item, &mut |blob| {
            captured_blob = Some(blob.clone());
            let id = new_persist_id();
            write_bytes_in(dir.path(), &id, &blob.bytes).expect("write");
            persisted_id = Some(id.clone());
            Some(id)
        });
        let id = persisted_id.expect("스크롤백이 디스크로 영속화되어야 한다");

        let captured_blob = captured_blob.expect("인라인 스크롤백이 저장 콜백에 전달돼야 한다");
        assert_eq!(captured_blob.lines, total, "값의 줄 수가 원본과 다르다");
        assert_eq!(
            captured_blob.bytes,
            serialize_lines(&legacy),
            "새 캡처 경로의 직렬화 결과가 옛 재압축 경로와 다르다"
        );
        let captured = decode_blob(&captured_blob).expect("인코딩된 값을 다시 풀 수 있어야 한다");
        assert_eq!(captured.len(), total);

        let got = expect_lines(dir.path(), &id);
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
            "기본값과 다른 속성이 있어야 직렬화 결과에서 속성도 비교할 수 있다"
        );
    }

    #[test]
    fn read_missing_reports_absent() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(matches!(
            read_in(dir.path(), "nonexistent"),
            ScrollbackRead::Absent
        ));
    }

    #[test]
    fn corrupt_file_reports_unreadable() {
        let dir = tempfile::tempdir().expect("tempdir");
        let id = new_persist_id();
        write_in(dir.path(), &id, &sample_lines()).expect("write");
        let path = dir.path().join(format!("{id}.{EXT}"));
        fs::write(&path, b"not a scrollback dump").expect("corrupt");

        assert!(matches!(
            read_in(dir.path(), &id),
            ScrollbackRead::Unreadable
        ));
        assert!(path.exists(), "못 읽은 파일을 리더가 지우지 않는다");
    }

    #[test]
    fn delete_is_idempotent() {
        let dir = tempfile::tempdir().expect("tempdir");
        let id = new_persist_id();
        write_in(dir.path(), &id, &sample_lines()).expect("write");
        delete_in(dir.path(), &id);
        delete_in(dir.path(), &id);
        assert!(matches!(read_in(dir.path(), &id), ScrollbackRead::Absent));
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

        assert!(matches!(
            read_in(dir.path(), &keep),
            ScrollbackRead::Loaded(_)
        ));
        assert!(matches!(read_in(dir.path(), &drop), ScrollbackRead::Absent));
    }

    #[test]
    fn clear_all_removes_everything() {
        let dir = tempfile::tempdir().expect("tempdir");
        let a = new_persist_id();
        let b = new_persist_id();
        write_in(dir.path(), &a, &sample_lines()).expect("write a");
        write_in(dir.path(), &b, &sample_lines()).expect("write b");
        gc_orphans_in(dir.path(), &HashSet::new());
        assert!(matches!(read_in(dir.path(), &a), ScrollbackRead::Absent));
        assert!(matches!(read_in(dir.path(), &b), ScrollbackRead::Absent));
    }

    #[test]
    fn rejects_invalid_persist_ids() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(write_in(dir.path(), "", &sample_lines()).is_err());
        assert!(write_in(dir.path(), "../escape", &sample_lines()).is_err());
        assert!(write_in(dir.path(), "with.dot", &sample_lines()).is_err());
    }
}
