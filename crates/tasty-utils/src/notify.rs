//! child→caller 완료 알림 로그 — Monitor tool 이 `tail -F` 로 감시하는 append-only 파일.
//!
//! 배경: child(claude/codex)가 끝났을 때 caller(conductor)에게 완료를 알리는 원래
//! 경로는 `terminal.tell` 로 PTY 에 텍스트를 강제 주입하는 것뿐이었다. 이 방식은
//! **caller 세션이 busy(다른 turn 생성 중)일 때 주입이 씹힌다** — conductor 가 무거운
//! 작업을 도는 동안 완료 알림을 놓치는 사고가 실제로 있었다. 게다가 caller 가 Claude
//! Code CLI 세션이면 주입된 텍스트가 **사람이 직접 타이핑한 발화와 구분되지 않는
//! 형태**로 대화 트랜스크립트에 섞여 들어간다.
//!
//! 처방(현재 유일 경로): 완료 이벤트를 **파일에 한 줄씩 append** 한다. conductor 는 dispatch
//! 직후 한 번 `Monitor({ command: "tail -n0 -F <path>", persistent: true })` 로 arm 하면,
//! 이후 child 완료마다 append 된 라인이 background-task notification 으로 **idle 세션도
//! 깨워** 다음 턴에 전달된다. 이 파일 채널이 안정적으로 검증된 뒤 완료-알림 경로에서
//! `terminal.tell` 주입은 **제거**했다(위 위장 발화 부작용 때문) — completion-log 가
//! 완료 알림의 유일한 채널이다.
//!
//! 경로 규약: `<parent_home>/notify/<caller_surface>.log`. host 는 자식(plugin
//! 서브프로세스 = writer, conductor 셸 = reader) 양쪽에 자기 데이터 루트를
//! **`TASTY_PARENT_HOME`** env 로 주입한다. [`notify_log_path`] 는 이 값을 최우선으로
//! 보고, 없을 때만 [`crate::path::tasty_home`] 으로 fallback 하므로 writer/reader 의
//! 경로가 항상 일치한다.
//!
//! `TASTY_HOME`(= `tasty_home()` 의 override 1순위)이 아니라 별도 이름을 쓰는 이유:
//! 정보성 부모-루트 값을 `TASTY_HOME` 으로 주입하면 release 터미널 안에서 실행된 debug
//! 빌드가 그 값을 자기 데이터 루트 override 로 오인해 `~/.tasty-debug` 격리가 깨지고
//! release 의 포트파일까지 덮어쓰는 사고가 났다. self-determination(`TASTY_HOME`)과
//! broadcast(`TASTY_PARENT_HOME`)를 환경변수 이름으로 분리한다.
//!
//! 라인 포맷은 완료 메시지 한 줄로 둔다 — 예: `surface 42 작업 완료 (호출 방식: spawn)`.
//! 호출 방식(spawn/tell)을 문장 맨 앞에 두지 않는 이유는 `crates/tasty-plugin-claude`/
//! `tasty-plugin-codex` 의 `notify_done_message`/`notify_caller_message` 주석 참조 —
//! "{command} 완료" 형태는 명령 자체가 끝났다는 뜻으로 오독되기 쉬웠다.

use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// 완료 로그 파일의 크기 상한. 초과 시 append 대신 truncate 하여 무한 성장을 막는다.
/// 완료 라인은 ~40 bytes 이므로 256 KiB ≈ 수천 개 이벤트분 — 한 세션에서 도달하기
/// 어렵지만, surface_id 가 세션 간 재사용되며 파일이 계속 누적되는 것을 방어한다.
const NOTIFY_LOG_CAP_BYTES: u64 = 256 * 1024;

/// caller_surface 별 완료 로그 파일 경로.
///
/// host 가 자식에 주입한 `TASTY_PARENT_HOME`(정보성 부모 루트)이 있으면 그 값을
/// 최우선으로 홈으로 쓰고, 없으면 [`crate::path::tasty_home`] 으로 fallback 한다.
/// writer(plugin)/reader(conductor) 양쪽 다 `TASTY_PARENT_HOME` 을 받으므로 이 함수가
/// 그걸 최우선으로 봐야 두 경로가 일치한다. 홈 해석 실패 시 `None`.
pub fn notify_log_path(caller_surface: u32) -> Option<PathBuf> {
    resolve_home(
        std::env::var("TASTY_PARENT_HOME").ok(),
        crate::path::tasty_home,
    )
    .map(|home| notify_log_path_in(&home, caller_surface))
}

/// 홈 루트 선택 로직(순수 — env 접근 없이 테스트 가능). `TASTY_PARENT_HOME` 값이
/// 비어있지 않으면 그것을, 아니면 `fallback`(보통 `tasty_home()`)을 쓴다. `fallback` 은
/// parent 가 없을 때만 호출하는 클로저라 불필요한 홈 해석을 피한다.
fn resolve_home(
    parent_env: Option<String>,
    fallback: impl FnOnce() -> Option<PathBuf>,
) -> Option<PathBuf> {
    if let Some(raw) = parent_env {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            return Some(PathBuf::from(trimmed));
        }
    }
    fallback()
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

    // notify_log_path() 의 홈 선택 로직은 pure `resolve_home` 로 추출돼 있어(이 crate 는
    // `#![forbid(unsafe_code)]` 라 테스트에서 env 를 직접 조작할 수 없다) env 조작 없이
    // 검증한다. resolve_home 이 곧 notify_log_path 가 쓰는 우선순위 규칙 전부다.

    // TASTY_PARENT_HOME 이 설정돼 있으면 그 값을 쓴다(fallback 은 호출조차 안 함).
    #[test]
    fn resolve_home_prefers_parent_env() {
        let got = resolve_home(Some("/tmp/fake-parent-home".to_string()), || {
            panic!("fallback must not run when parent env is set")
        });
        assert_eq!(got, Some(PathBuf::from("/tmp/fake-parent-home")));
    }

    // 이를 notify_log_path 경로 조립까지 연결하면 최종 경로가 parent home 밑에 놓인다.
    #[test]
    fn parent_env_drives_full_notify_path() {
        let home = resolve_home(Some("/tmp/fake-parent-home".to_string()), || None).unwrap();
        assert_eq!(
            notify_log_path_in(&home, 9),
            PathBuf::from("/tmp/fake-parent-home/notify/9.log")
        );
    }

    // TASTY_PARENT_HOME 이 없으면 fallback(= tasty_home())을 쓴다.
    #[test]
    fn resolve_home_falls_back_when_parent_absent() {
        let got = resolve_home(None, || Some(PathBuf::from("/tmp/fallback-root")));
        assert_eq!(got, Some(PathBuf::from("/tmp/fallback-root")));
    }

    // 빈/공백 값도 미설정으로 간주하고 fallback 한다.
    #[test]
    fn resolve_home_treats_empty_parent_as_absent() {
        let got = resolve_home(Some("   ".to_string()), || {
            Some(PathBuf::from("/tmp/fallback-root2"))
        });
        assert_eq!(got, Some(PathBuf::from("/tmp/fallback-root2")));
    }

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
        append_line_to(&path, "surface 7 작업 완료 (호출 방식: spawn)", 1024).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "surface 7 작업 완료 (호출 방식: spawn)\n");
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
