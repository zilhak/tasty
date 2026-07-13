//! child→caller 완료 알림 로그 — Monitor tool 이 `tail -F` 로 감시하는 append-only 파일.
//!
//! 배경: child(claude/codex)가 끝났을 때 caller(conductor)에게 완료를 알리는 기존
//! 경로는 `terminal.tell` 로 PTY 에 텍스트를 강제 주입하는 것뿐이었다. 이 방식은
//! **caller 세션이 busy(다른 turn 생성 중)일 때 주입이 씹힌다** — conductor 가 무거운
//! 작업을 도는 동안 완료 알림을 놓치는 사고가 실제로 있었다.
//!
//! 처방(추가 경로): 완료 이벤트를 **파일에 한 줄씩 append** 한다. conductor 는 dispatch
//! 직후 한 번 `Monitor({ command: "tail -n0 -F <path>", persistent: true })` 로 arm 하면,
//! 이후 child 완료마다 append 된 라인이 background-task notification 으로 **idle 세션도
//! 깨워** 다음 턴에 전달된다. `terminal.tell` 은 fallback 으로 유지한다(제거하지 않음).
//!
//! 경로 규약: `<tasty_home>/notify/<caller_surface>.log`. conductor 는 자기 surface_id
//! (`TASTY_SURFACE_ID`)와 `tasty_home()` 으로 이 경로를 직접 구성할 수 있다. plugin 은
//! 호스트가 주입한 `TASTY_HOME` env 로 [`crate::path::tasty_home`] 이 host 와 동일 루트를
//! 반환하므로 writer(plugin)와 reader(conductor)의 경로가 항상 일치한다.
//!
//! 라인 포맷은 caller 에게 tell 로 주입하던 메시지와 **동일 문자열**로 둔다(divergence
//! 방지) — 예: `spawn 완료: surface 42`.

use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// 완료 로그 파일의 크기 상한. 초과 시 append 대신 truncate 하여 무한 성장을 막는다.
/// 완료 라인은 ~40 bytes 이므로 256 KiB ≈ 수천 개 이벤트분 — 한 세션에서 도달하기
/// 어렵지만, surface_id 가 세션 간 재사용되며 파일이 계속 누적되는 것을 방어한다.
const NOTIFY_LOG_CAP_BYTES: u64 = 256 * 1024;

/// caller_surface 별 완료 로그 파일 경로. `tasty_home()` 이 없으면(홈 해석 실패) `None`.
pub fn notify_log_path(caller_surface: u32) -> Option<PathBuf> {
    crate::path::tasty_home().map(|home| notify_log_path_in(&home, caller_surface))
}

/// 순수 경로 조립(파일시스템 접근 없음) — 단위 테스트 대상.
fn notify_log_path_in(home: &Path, caller_surface: u32) -> PathBuf {
    home.join("notify").join(format!("{caller_surface}.log"))
}

/// caller_surface 의 완료 로그에 한 줄 append 한다(개행 자동 부가). `notify/` 디렉토리는
/// 없으면 생성한다. best-effort — 호출자는 실패 시 `tracing::warn!` 로 흘려보내고 기존
/// `terminal.tell` 알림 경로에는 영향을 주지 않는다.
pub fn append_notify_line(caller_surface: u32, line: &str) -> io::Result<()> {
    let path = notify_log_path(caller_surface)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "tasty_home() unavailable"))?;
    append_line_to(&path, line, NOTIFY_LOG_CAP_BYTES)
}

/// 실제 append/truncate 로직 — 경로·cap 을 명시로 받아 env 조작 없이 테스트 가능.
fn append_line_to(path: &Path, line: &str, cap: u64) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    // 기존 파일이 cap 이상이면 새로 시작(truncate). `tail -F` 는 파일 축소를 감지해
    // 재오픈하므로 arm 된 Monitor 가 truncate 후 append 된 라인을 계속 받는다.
    let truncate = std::fs::metadata(path)
        .map(|m| m.len() >= cap)
        .unwrap_or(false);
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(!truncate)
        .truncate(truncate)
        .open(path)?;
    writeln!(file, "{line}")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_in_builds_notify_subdir_and_log_suffix() {
        let home = Path::new("/home/u/.tasty");
        assert_eq!(
            notify_log_path_in(home, 42),
            PathBuf::from("/home/u/.tasty/notify/42.log")
        );
    }

    #[test]
    fn append_creates_dir_and_writes_line_with_newline() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("notify").join("7.log");
        append_line_to(&path, "spawn 완료: surface 7", 1024).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "spawn 완료: surface 7\n");
    }

    #[test]
    fn append_accumulates_multiple_lines() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("a.log");
        append_line_to(&path, "first", 1024).unwrap();
        append_line_to(&path, "second", 1024).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "first\nsecond\n");
    }

    #[test]
    fn append_truncates_when_over_cap() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("cap.log");
        // cap=8: "aaaaaa\n" = 7 bytes < 8 이므로 첫 줄은 남고 다음 append 전 크기는 7.
        append_line_to(&path, "aaaaaa", 8).unwrap();
        // 이제 파일 크기 7 < 8 → 두 번째는 append. 크기 7+"bbbbbb\n"(7)=14 ≥ 8.
        append_line_to(&path, "bbbbbb", 8).unwrap();
        // 세 번째 append 시점: 기존 14 ≥ cap 8 → truncate 후 이 줄만 남는다.
        append_line_to(&path, "cccccc", 8).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "cccccc\n");
    }
}
