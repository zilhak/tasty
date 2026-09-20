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
//! # 보존 범위 — 이 로그가 답할 수 있는 물음의 크기
//!
//! 이 파일은 되감을 수 있는 기록이 아니다. 무엇이 남는지를 세 축으로 못박는다.
//!
//! - **정체성** — 한 로그의 정체성은 경로 그 자체, 즉 **(데이터 루트, caller surface
//!   id)** 다. 그 외에 인스턴스를 가리키는 표식은 줄에도 파일에도 없다. 데이터 루트는
//!   `TASTY_PARENT_HOME`(없으면 [`crate::path::tasty_home`])이 정하므로, 그 값이 다르면
//!   **같은 surface 번호라도 다른 로그**이고 같으면 같은 로그다. writer 가 그 env 없이
//!   돌면 자기 홈으로 떨어져 reader 와 조용히 갈린다 — [`resolve_home`] 의 fallback 이
//!   그 자리다.
//! - **세대** — surface id 는 호스트 실행마다 1 부터 다시 발급된다. 그래서 이전 실행이
//!   남긴 파일과 이번 실행의 파일은 **이름이 겹친다.** 겹침을 막는 것은 파일 안의
//!   표식이 아니라 **호스트가 부팅 때 `notify/` 를 통째로 지우는 것** 하나다. 즉 이
//!   로그의 보존 범위는 **지금 호스트 세대 하나**이고, 재시작을 사이에 두고 과거 줄을
//!   되읽을 방법은 없다. 그 삭제가 데이터 루트 단위라는 점이 곧 전제다 — **한 데이터
//!   루트에 호스트 하나.** 둘이 같은 루트로 뜨면 나중 것이 먼저 것의 살아 있는 로그를
//!   지운다.
//! - **크기** — 축은 **바이트 하나**다([`NOTIFY_LOG_CAP_BYTES`]). 시간 상한도, 파일 수
//!   상한도 없다. 닫힌 surface 의 파일은 그 세대가 끝날 때까지 남는다(회수는 다음 부팅
//!   삭제뿐). 그리고 cap 은 "마지막 256 KiB 를 남긴다" 가 아니라 **"넘으면 전량
//!   버린다"** 다 — 남는 양은 0 과 cap 사이를 톱니로 오간다.
//!
//! 버리는 쪽은 **숨기지 않는다.** 비울 때마다 버린 바이트 수를 로그(`tracing`)로
//! 남긴다 — 이 파일 자신에는 안 쓴다. 읽는 쪽 계약이 "한 줄 = 완료 통지" 라 메타 줄을
//! 끼우면 그것이 완료로 읽히기 때문이다(ADR-0330 이 그 대안을 기각한 자리). 근거·측정·
//! 재검토 조건은 `docs/adr/0344-the-completion-log-keeps-one-host-generation-and-says-what-it-threw-away.md`.
//!
//! 라인 포맷은 완료 메시지 한 줄로 둔다 — 예: `surface 42 작업 완료 (호출 방식: spawn)`.
//! 호출 방식(spawn/tell)을 문장 맨 앞에 두지 않는 이유는 `crates/tasty-plugin-claude`/
//! `tasty-plugin-codex` 의 `notify_done_message`/`notify_caller_message` 주석 참조 —
//! "{command} 완료" 형태는 명령 자체가 끝났다는 뜻으로 오독되기 쉬웠다.

use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// 완료 로그 파일의 크기 상한. 무한 성장을 막는 **유일한** 축이다 — 시간 상한도
/// 파일 수 상한도 없다(모듈 문서 "보존 범위").
///
/// 도달하면 남기는 것이 아니라 **전량 버린다**: "마지막 256 KiB 를 보존" 이 아니라
/// "256 KiB 에서 0 으로 되돌린다" 이므로, 실제 보존량은 0 과 이 값 사이를 오간다.
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
///
/// # 이 파일에는 writer 가 여럿이다
///
/// 한 caller 밑에 claude child 와 codex child 가 함께 뜨면 **서로 다른 두 프로세스**가
/// 같은 `<caller_surface>.log` 에 쓴다. 그래서 이 함수의 불변식은 "한 줄은 통째로
/// 남거나 통째로 없다" 이고, 그것을 두 가지로 지킨다.
///
/// - **핸들은 항상 append 다.** 쓰기 핸들에 `truncate` 를 섞지 않는다. 섞으면 그
///   핸들은 offset 0 부터 쓰므로 다른 writer 가 방금 append 한 줄의 앞부분을 덮어
///   **양쪽 모두 아닌 잔해**를 남긴다.
/// - **한 줄은 한 번의 `write` 로 보낸다.** `writeln!` 은 포맷 조각마다 write 를
///   나눠 보낼 수 있고, 그 사이에 다른 프로세스의 append 가 끼면 두 줄이 섞인다.
///   `O_APPEND` 가 보장하는 것은 **한 번의 write** 가 끝에 통째로 붙는 것뿐이다.
///
/// 비우는 일은 쓰기와 **분리한다.** 판정과 실행 사이에 다른 writer 가 이미 비웠을 수
/// 있어 같은 핸들로 한 번 더 재고 비운다. 그래도 남는 창이 있다 — 비우기 직전에
/// append 된 줄은 사라진다. 그 줄은 파일이 이미 cap 을 넘은 뒤에 쓰인 것이라
/// **애초에 이 truncate 가 버릴 구간**이고, 손실의 범위가 "cap 을 넘은 시점 이전" 으로
/// 정의된다는 뜻이다. 사라지는 것은 줄 단위이고 줄 중간이 잘리지는 않는다.
fn append_line_to(path: &Path, line: &str, cap: u64) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    // 기존 파일이 cap 이상이면 새로 시작. `tail -F` 는 파일 축소를 감지해 재오픈하므로
    // arm 된 Monitor 가 그 뒤 append 된 라인을 계속 받는다.
    if std::fs::metadata(path)
        .map(|m| m.len() >= cap)
        .unwrap_or(false)
        && let Some(discarded) = truncate_over_cap(path, cap)
    {
        // 버린 양을 **로그에** 남긴다 — 이 파일 자신에는 안 쓴다. 읽는 쪽 계약은
        // "한 줄 = 완료 통지" 이므로 여기에 메타 줄을 끼우면 그 줄이 완료로 읽힌다
        // (ADR-0330 이 그 대안을 기각한 이유). 그렇다고 아무 데도 안 남기면 뒤처진
        // reader 의 미독분이 통째로 사라진 사실을 아무도 설명하지 못한다.
        tracing::warn!(
            "notify log {} hit the {cap}-byte cap — discarded {discarded} bytes of completion lines, \
             including anything a lagging reader had not read yet",
            path.display()
        );
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    let mut record = String::with_capacity(line.len() + 1);
    record.push_str(line);
    record.push('\n');
    file.write_all(record.as_bytes())
}

/// cap 을 넘은 파일을 비운다 — **쓰기 핸들과 분리된 자리**다.
///
/// 연 핸들로 크기를 **다시 재는** 이유: 바깥의 `metadata()` 판정과 여기 사이에 다른
/// writer 가 이미 비웠을 수 있다. 그때 또 비우면 그 writer 가 새로 쓴 줄들을 지운다.
///
/// 반환값은 **실제로 버린 바이트 수**다. `None` 은 안 비웠다는 뜻이고 갈래가 셋이다 —
/// 못 열었거나, 다시 재 보니 이미 cap 아래였거나, `set_len` 이 실패했다. 셋 다 "버린
/// 것이 없다" 이므로 호출자가 손실을 보고해서는 안 된다. 이 값을 돌려주는 이유는
/// 손실의 **양**이 여기서만 알려지기 때문이다: 비우고 나면 파일이 0 이라 나중에
/// 아무도 얼마가 사라졌는지 되물을 수 없다.
fn truncate_over_cap(path: &Path, cap: u64) -> Option<u64> {
    let opened = std::fs::OpenOptions::new().write(true).open(path);
    // 못 열면 여기서 보고하지 않는다 — 곧바로 이어지는 append 가 같은 원인으로
    // 실패해 호출자에게 `io::Error` 로 올라간다. 두 번 시끄럽게 할 이유가 없다.
    let file = opened.ok()?;
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    if len < cap {
        return None;
    }
    if let Err(e) = file.set_len(0) {
        // best-effort — 못 비워도 append 는 그대로 진행한다(파일이 cap 을 넘어 자랄
        // 뿐 알림은 계속 간다). 조용히 넘기면 그 성장을 아무도 설명하지 못한다.
        tracing::warn!("notify log truncate failed for {}: {e}", path.display());
        return None;
    }
    Some(len)
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

    // 이 파일에는 writer 가 여럿이라, cap 경계에서 겹쳐도 **한 줄이 통째로 남거나
    // 통째로 없어야** 한다. 그 불변식을 여기서 잰다.
    //
    // 한계: 여기 재는 것은 같은 프로세스의 두 스레드다. 실제 상황은 claude child 와
    // codex child 가 **서로 다른 프로세스**로 붙는 것이고, 프로세스 둘로 재면 같은
    // 결함이 훨씬 크게 드러난다. 두 프로세스를 띄우는 하네스는 이 크레이트 안에
    // 둘 자리가 없어(새 타깃이 필요하다) 스레드 판을 둔다 — 그래도 고치기 전
    // 코드에서는 이 판도 깨진다.
    //
    // 두 writer 의 줄 길이를 다르게 두는 것이 핵심이다. 길이가 같으면 덮어쓰기가
    // 같은 폭을 채워 흔적이 안 남는다.
    #[test]
    fn concurrent_writers_never_leave_a_partial_line() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("race.log");
        // cap 은 이 시험이 **truncate 갈래를 지나지 않을 만큼** 높게 둔다. 낮추면 뒤의
        // truncate 가 앞에서 생긴 잔해를 지워 시험이 간헐적으로 통과한다 — 고치기 전
        // 코드에 대고 재 보면 cap 512 에서는 3/3 통과, 64 KiB 에서는 1/3 통과였고
        // 여기 값에서만 3/3 실패였다. 그래서 이 시험이 재는 것은 **한 줄이 한 번의
        // write 로 나가는가** 하나이고, truncate 갈래의 경합은 이것으로 안 재진다.
        const CAP: u64 = 1 << 30;
        const ROUNDS: usize = 2_000;
        // writer 를 둘만 두면 겹치는 창이 좁다. 실제 상황(claude child + codex child)
        // 보다 많이 띄워 창을 넓힌다.
        const WRITERS_PER_SHAPE: usize = 4;
        let long = "X".repeat(60);
        std::thread::scope(|scope| {
            for _ in 0..WRITERS_PER_SHAPE {
                scope.spawn(|| {
                    for i in 0..ROUNDS {
                        append_line_to(&path, &format!("A-{i:06}"), CAP)
                            .expect("append_line_to failed");
                    }
                });
                scope.spawn(|| {
                    for i in 0..ROUNDS {
                        append_line_to(&path, &format!("B-{i:06}-{long}"), CAP)
                            .expect("append_line_to failed");
                    }
                });
            }
        });
        let text = std::fs::read_to_string(&path).unwrap();
        let malformed: Vec<&str> = text
            .lines()
            .filter(|l| {
                let a = l.len() == 8 && l.starts_with("A-");
                let b = l.len() == 69 && l.starts_with("B-");
                !(a || b)
            })
            .collect();
        assert!(
            malformed.is_empty(),
            "둘 중 누구의 줄도 아닌 잔해가 {} 줄 남았다 — 덮어썼거나 한 줄이 여러 번의 \
             write 로 쪼개져 끼어들었다는 뜻이다. 처음 셋: {:?}",
            malformed.len(),
            &malformed[..malformed.len().min(3)]
        );
    }

    // 비우기는 **얼마를 버렸는지**를 값으로 돌려준다. 비우고 나면 파일이 0 이라
    // 그 양을 나중에 되물을 방법이 없고, 그 수가 없으면 호출자가 손실을 로그에
    // 적을 수 없다 — "유실을 숨기지 않는다" 가 이 반환값 위에 서 있다.
    #[test]
    fn truncate_reports_how_many_bytes_it_threw_away() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("over.log");
        std::fs::write(&path, vec![b'x'; 300]).unwrap();
        assert_eq!(truncate_over_cap(&path, 256), Some(300));
        assert_eq!(std::fs::metadata(&path).unwrap().len(), 0);
    }

    // 안 버렸으면 아무 수도 돌려주지 않는다. 이 갈래는 실제로 일어난다 — 바깥의
    // `metadata()` 판정과 여기 사이에 다른 writer 가 이미 비웠을 수 있다. 그때
    // `Some(0)` 을 돌려주면 호출자가 "0 바이트를 버렸다" 를 로그에 적어, 아무 일도
    // 없던 자리에 손실 기록이 남는다.
    #[test]
    fn truncate_reports_nothing_when_another_writer_already_emptied_it() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("raced.log");
        std::fs::write(&path, b"short\n").unwrap();
        assert_eq!(truncate_over_cap(&path, 256), None);
        assert_eq!(std::fs::metadata(&path).unwrap().len(), 6);
    }

    // 없는 파일은 열리지 않으므로 버린 것도 없다. 이어지는 append 가 같은 원인으로
    // 실패해 호출자에게 `io::Error` 로 올라가므로 여기서 또 보고하지 않는다.
    #[test]
    fn truncate_reports_nothing_when_the_file_cannot_be_opened() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("absent.log");
        assert_eq!(truncate_over_cap(&path, 1), None);
    }

    // cap 도달은 **파일 전체**를 버린다 — 넘긴 만큼만 잘라 내는 것이 아니다. 뒤처진
    // reader 의 미독분도 그 안에 있고, 그것이 이 로그의 보존 범위가 "지금 세대의
    // 마지막 비우기 이후" 인 이유다.
    #[test]
    fn hitting_the_cap_discards_the_whole_file_not_just_the_excess() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("whole.log");
        for i in 0..20 {
            append_line_to(&path, &format!("unread-{i:02}"), 64).unwrap();
        }
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(
            content.lines().count() < 20,
            "cap 을 넘겼는데도 20 줄이 다 남았다 — 비우기가 안 돌았다"
        );
        assert!(
            !content.contains("unread-00"),
            "첫 줄이 남아 있다 — 비우기가 초과분만 잘라냈다는 뜻이고, 그러면 보존 \
             범위를 '마지막 비우기 이후' 로 말할 수 없다: {content:?}"
        );
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
