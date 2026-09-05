//! agent hook 전달 실패의 IPC-독립 기록.
//!
//! Claude Code / Codex CLI 는 턴 경계마다 `tasty claude hook <event>` /
//! `tasty codex hook <event>` 를 동기 실행해 상태 전환을 tasty 로 **1 회성 push**
//! 한다. 재전송이 없으므로 **유실 = 영구 손실**인데, 셸 래퍼가 exit code 를 버려
//! (`|| true`) 유실됐다는 사실 자체가 어디에도 남지 않았다 — 자식이 영구히
//! `active` 로 남은 사고에서 원인을 특정하지 못한 이유가 이것이다.
//!
//! **왜 IPC 가 아니라 파일인가**: 실패의 주된 원인이 "tasty 에 닿지 못함" 이므로,
//! `telemetry.record` 같은 IPC 채널로는 그 실패를 보고할 수 없다(chicken-and-egg).
//! 그래서 프로세스 로컬 append-only 파일에 남긴다.
//!
//! **어느 홈에 남기는가**: `tasty_home()`(=`TASTY_HOME` 또는 `~/.tasty{-debug}`)
//! 아래다. CLI 가 접속을 시도하는 대상은 `port_file` 이 가리키는 인스턴스이고 그
//! 포트 파일 위치가 `tasty_home()` 이므로, 기록은 **닿으려 했던 그 인스턴스의 홈**에
//! 남아야 사후 대조가 된다. `TASTY_PARENT_HOME`(부모가 자식 셸에 브로드캐스트하는
//! 데이터 루트)은 여기서 쓰지 않는다 — 접속 대상 결정에 관여하지 않는 값이라 섞으면
//! 기록과 대상 인스턴스가 어긋난다.
//!
//! **best-effort**: 기록 자체가 실패해도(권한/디스크/홈 미확정) 무시한다. 진단
//! 로그를 남기려다 hook 을 깨뜨리면 본말전도다.

use std::io::Write;
use std::path::PathBuf;

/// 기록 파일명. 기존 `notify/<surface>.log` 와 같은 성격(저비용 append-only)이다.
const LOG_FILE: &str = "hook-failures.log";

/// 로테이션 임계치. 넘으면 `<name>.1` 로 밀어내고 새 파일을 시작한다 — 보존 상한은
/// 이 값의 2 배(현재 파일 + `.1`). hook 실패는 정상 환경에서 0 건이고, 고장 상황에서도
/// 턴당 한 줄(수십 바이트)이라 이 정도면 사고 한 건의 전체 이력을 담고도 남는다.
const MAX_BYTES: u64 = 256 * 1024;

/// 이 method 가 **agent hook 전달**인가.
///
/// 기록 대상을 hook 으로 좁히는 이유: 대화형 CLI 실패는 사용자가 stderr 로 즉시
/// 보므로 무흔적 문제가 없다. 반면 hook 은 아무도 안 보는 곳에서 발화하고 재시도가
/// 없다 — 이 파일이 유일한 흔적이 된다.
///
/// 판정은 method 의 마지막 dot 세그먼트가 `hook` 이거나 `_hook` 으로 끝나는지로 한다:
/// `claude.hook` / `codex.hook` / `claude.checklist_hook` 이 대상이다. plugin 이
/// 새 hook 명령을 추가해도 이 명명 관례만 지키면 자동으로 포함된다.
pub fn is_hook_method(method: &str) -> bool {
    let tail = method.rsplit('.').next().unwrap_or(method);
    tail == "hook" || tail.ends_with("_hook")
}

fn log_path() -> Option<PathBuf> {
    tasty_utils::path::tasty_home().map(|d| d.join(LOG_FILE))
}

/// UTC ISO-8601(초 단위). 외부 크레이트 없이 `SystemTime` 에서 직접 만든다 —
/// 이 크레이트에 시간 포맷팅 의존성을 새로 들이지 않기 위함.
fn utc_timestamp() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let (y, mo, d) = civil_from_days(days);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
}

/// days-since-epoch → (year, month, day). Howard Hinnant 의 `civil_from_days`
/// 알고리즘(public domain) — 윤년/윤세기를 분기 없이 처리한다.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// hook 이벤트 이름을 params 에서 꺼낸다. 없으면 `-`.
///
/// `method` 는 `claude.hook` 하나로 고정이라 **어느 이벤트가 실패했는지**는 거기
/// 안 들어 있다 — 실제 이벤트(`stop`/`session-end`/…)는 `params.event` 에 실린다.
/// 로그를 읽는 사람이 가장 먼저 묻는 질문이 그것이므로 여기서 꺼내 함께 싣는다.
/// 이벤트 이름을 갖지 않는 hook(`claude.checklist_hook`)은 `-` 가 된다.
///
/// 공백은 `_` 로 접는다: `event` 는 줄 중간 필드라 공백이 들어가면 `key=value`
/// 나열이 깨져 `awk '{print $3}'` 같은 읽기가 어긋난다(마지막 필드인 `reason` 과
/// 다른 제약이다).
fn event_token(params: &serde_json::Value) -> String {
    let raw = params.get("event").and_then(|v| v.as_str()).unwrap_or("");
    let folded: String = raw
        .chars()
        .map(|c| if c.is_whitespace() { '_' } else { c })
        .collect();
    if folded.is_empty() {
        "-".into()
    } else {
        folded
    }
}

/// `reason` 에 실을 수 있는 값 — **CLI 가 만든, 로케일과 무관한 영어 진단 문장**.
///
/// 이 파일을 읽는 주체는 사람일 수도 에이전트일 수도 있다. 알려진 실패 패턴과 대조하고
/// `grep` 으로 찾으려면 흔들리지 않는 조각이 있어야 한다. 반면 stderr 는 사용자 표면이라
/// 번역문이 맞다 — **같은 실패의 두 산출물이 언어를 달리한다.**
///
/// **이 타입이 덮는 것은 CLI 가 문구를 만드는 갈래뿐이다.** 요청이 호스트에 닿았는데
/// 오류 응답이 온 갈래는 문구를 답한 쪽이 만들고, plugin 이 답하면 그 문구는 앱 언어를
/// 탄다 — CLI 에 영어 원본이 없으므로 갈라 놓을 것도 없다. 그 갈래의 로케일 무관성은
/// 산문이 아니라 `code=` 필드가 진다([`format_line`]).
///
/// 그 분리를 주석이 아니라 **타입**으로 세운 이유: [`record`] 가 `&str` 을 받으면 다음에
/// 손대는 사람이 `t(...)` 결과를 그대로 넘기는 것을 막을 방법이 없고, 실제로 두 lane 이
/// 각각 독립적으로 그렇게 만들었다. 지금은 번역문을 실으려면 [`Self::new_unchecked`] 를
/// 명시적으로 불러야 하므로, 그 한 줄이 리뷰에서 보인다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticEnglish(String);

impl DiagnosticEnglish {
    /// 로케일 무관 영어임을 **호출자가 보증**하고 만든다.
    ///
    /// 쓸 수 있는 값: 크레이트가 `Display` 로 내는 영어 기본 렌더링(`PortFileError` 등),
    /// 코드에 리터럴로 박은 영어 포맷, 그리고 **답한 쪽이 만들어 보낸 오류 문구**.
    /// 마지막 것은 CLI 가 언어를 고를 수 없는 값이라 보증의 대상이 아니다 — 그 갈래에서
    /// 흔들리지 않는 것은 함께 기록하는 `code` 다.
    /// **쓰면 안 되는 값**: CLI 자신이 `t()` / `t_fmt()` 로 만든 문구.
    pub fn new_unchecked(text: impl Into<String>) -> Self {
        Self(text.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for DiagnosticEnglish {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// 한 줄 기록. 공백을 구분자로 쓰는 `key=value` 나열이라 `grep`/`awk` 로 바로 읽힌다.
/// `reason` 은 CLI 가 이미 만들어 둔 실패 메시지를 그대로 싣는다 — 없는 정보를 새로
/// 만드는 게 아니라, 버려지던 정보를 붙잡아 두는 것이 이 모듈의 전부다. `event` 도
/// 같은 성격이다: 요청에 이미 있던 값을 흘려보내지 않고 붙잡아 둘 뿐이다.
///
/// `code` 는 JSON-RPC 오류 코드다. **좌표는 필드로, 산문은 `reason` 으로** 가른다 —
/// 코드는 프로토콜 값이라 로케일을 안 타는 반면 `reason` 은 답한 쪽이 만든 문장이라
/// 탈 수 있다. 그 값이 `reason` 앞머리에 묻혀 있으면 읽는 쪽이 산문을 파싱해야 하고,
/// 그건 이 파일이 피하려던 바로 그 형태다. 코드가 없는 실패(호스트에 닿지도 못한
/// 경우)는 `-` 다 — `event` 와 같은 부재 표기다.
///
/// **`reason` 은 계속 마지막이다.** 한 줄에서 공백을 담을 수 있는 값이 그것뿐이라,
/// 읽는 쪽이 `reason=` 뒤를 줄 끝까지로 자를 수 있다는 성질을 깨지 않는다.
fn format_line(
    method: &str,
    params: &serde_json::Value,
    code: Option<i32>,
    reason: &str,
) -> String {
    let surface = std::env::var("TASTY_SURFACE_ID").unwrap_or_else(|_| "-".into());
    let event = event_token(params);
    let code = code.map_or_else(|| "-".to_string(), |c| c.to_string());
    // 줄 단위 레코드이므로 개행은 공백으로 접는다(한 실패 = 한 줄 불변식).
    let reason = reason.replace(['\n', '\r'], " ");
    format!(
        "{ts} method={method} event={event} surface={surface} code={code} reason={reason}\n",
        ts = utc_timestamp()
    )
}

/// hook 전달 실패를 기록한다. hook 이 아닌 method 는 무시한다.
///
/// `reason` 이 [`DiagnosticEnglish`] 인 것은 규약이다 — 이 파일의 문구는 사용자 로케일을
/// 따라가지 않는다. 사용자에게 보여줄 번역문은 호출자가 stderr 로 따로 낸다.
///
/// 실패해도 조용히 넘어간다(best-effort) — 호출자는 반환값을 볼 필요가 없다.
pub fn record(
    method: &str,
    params: &serde_json::Value,
    code: Option<i32>,
    reason: &DiagnosticEnglish,
) {
    let Some(path) = log_path() else { return };
    record_at(&path, method, params, code, reason.as_str());
}

/// 경로를 주입받는 실제 구현 — 테스트가 프로세스 전역 env(`TASTY_HOME`)를 건드리지
/// 않고 검증할 수 있게 분리했다(같은 프로세스에서 병렬로 도는 다른 테스트와의 경쟁 방지).
fn record_at(
    path: &std::path::Path,
    method: &str,
    params: &serde_json::Value,
    code: Option<i32>,
    reason: &str,
) {
    if !is_hook_method(method) {
        return;
    }
    if let Some(parent) = path.parent()
        && std::fs::create_dir_all(parent).is_err()
    {
        return;
    }
    rotate_if_needed(path);
    // append-only. 열기/쓰기 실패는 그대로 포기한다 — 진단 기록이 hook 을 깨뜨리면 안 된다.
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = f.write_all(format_line(method, params, code, reason).as_bytes()); // best-effort
    }
}

/// 임계치를 넘었으면 `<name>.1` 로 밀어낸다. rename 실패는 무시 — 다음 기록이 그냥
/// 커진 파일에 이어 붙을 뿐이라 관측 가능성 자체는 유지된다.
fn rotate_if_needed(path: &std::path::Path) {
    let too_big = std::fs::metadata(path)
        .map(|m| m.len() >= MAX_BYTES)
        .unwrap_or(false);
    if too_big {
        let _ = std::fs::rename(path, path.with_extension("log.1")); // best-effort
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hook_methods_are_recognized() {
        assert!(is_hook_method("claude.hook"));
        assert!(is_hook_method("codex.hook"));
        assert!(is_hook_method("claude.checklist_hook"));
    }

    /// 대화형/조회 명령은 기록 대상이 아니다 — 실패가 stderr 로 사용자에게 바로
    /// 보이므로 무흔적 문제가 없고, 파일만 시끄러워진다.
    #[test]
    fn non_hook_methods_are_ignored() {
        assert!(!is_hook_method("claude.children"));
        assert!(!is_hook_method("terminal.spawn"));
        assert!(!is_hook_method("claude.install"));
        // `hook` 이 앞쪽에 있을 뿐인 method 도 대상이 아니다.
        assert!(!is_hook_method("hook.set"));
        assert!(!is_hook_method("claude.hook_status"));
    }

    #[test]
    fn line_is_single_line_and_greppable() {
        let line = format_line(
            "claude.hook",
            &serde_json::json!({ "event": "stop" }),
            Some(-32602),
            "boom\nsecond line",
        );
        assert_eq!(line.matches('\n').count(), 1, "한 실패 = 한 줄");
        assert!(line.ends_with('\n'));
        assert!(line.contains("method=claude.hook"));
        assert!(line.contains("reason=boom second line"));
    }

    /// 로그를 읽는 사람의 첫 질문("어떤 이벤트가 실패했나")에 답하는 필드다.
    /// `method` 는 `claude.hook` 하나로 고정이라 이게 없으면 `stop` 실패와
    /// `session-end` 실패를 구분할 수 없다.
    #[test]
    fn event_token_is_carried_from_params() {
        for event in ["stop", "notification", "session-end", "subagent-stop"] {
            let line = format_line(
                "claude.hook",
                &serde_json::json!({ "event": event }),
                None,
                "could not connect",
            );
            assert!(line.contains(&format!("event={event}")), "{line}");
        }
    }

    /// 이벤트 이름을 갖지 않는 hook(`claude.checklist_hook`)이나 params 가 비었을
    /// 때도 필드 자리는 유지한다 — 필드가 통째로 빠지면 열 기준으로 읽던 쪽이 어긋난다.
    #[test]
    fn missing_event_becomes_dash() {
        for params in [
            serde_json::json!({}),
            serde_json::json!({ "session_id": "abc" }),
            serde_json::json!({ "event": "" }),
            // 문자열이 아닌 값도 `-` 로 떨어뜨린다(있는 척하지 않는다).
            serde_json::json!({ "event": 3 }),
            serde_json::Value::Null,
        ] {
            let line = format_line("claude.checklist_hook", &params, None, "no port file");
            assert!(line.contains("event=-"), "{line}");
        }
    }

    /// `event` 는 줄 중간 필드라 공백이 섞이면 `key=value` 나열이 깨진다.
    #[test]
    fn event_whitespace_is_folded() {
        let line = format_line(
            "claude.hook",
            &serde_json::json!({ "event": "we ird\nname" }),
            None,
            "boom",
        );
        assert_eq!(line.matches('\n').count(), 1, "한 실패 = 한 줄");
        assert!(line.contains("event=we_ird_name"), "{line}");
        // 필드 개수가 늘지 않았는지 = 공백 구분 파싱이 그대로 먹히는지.
        let fields: Vec<&str> = line.trim_end().split(' ').collect();
        assert_eq!(fields[1], "method=claude.hook");
        assert_eq!(fields[2], "event=we_ird_name");
        assert!(fields[3].starts_with("surface="));
    }

    /// epoch/알려진 날짜로 달력 변환을 고정한다(윤년 포함).
    #[test]
    fn timestamp_calendar_conversion() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(59), (1970, 3, 1));
        // 2024-02-29 (윤년) = 19782 days since epoch.
        assert_eq!(civil_from_days(19782), (2024, 2, 29));
    }

    /// 기록 → 파일에 한 줄 append, hook 이 아닌 method 는 무시, 임계치 초과 시 로테이션.
    #[test]
    fn record_appends_and_rotates() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(LOG_FILE);

        let stop = serde_json::json!({ "event": "stop" });
        record_at(&path, "claude.hook", &stop, None, "could not connect");
        record_at(&path, "claude.children", &stop, None, "ignored");
        let body = std::fs::read_to_string(&path).expect("log written");
        assert_eq!(body.lines().count(), 1, "hook 실패만 기록된다");
        assert!(body.contains("method=claude.hook"));
        assert!(body.contains("event=stop"), "{body}");

        // 임계치를 넘긴 파일은 다음 기록 전에 `.1` 로 밀린다.
        std::fs::write(&path, vec![b'x'; MAX_BYTES as usize]).expect("grow");
        record_at(
            &path,
            "codex.hook",
            &serde_json::json!({ "event": "session-end" }),
            None,
            "timed out",
        );
        assert!(dir.path().join("hook-failures.log.1").exists(), "로테이션");
        let body = std::fs::read_to_string(&path).expect("new log");
        assert_eq!(body.lines().count(), 1, "새 파일은 새 줄만");
    }

    /// **좌표는 필드로, 산문은 `reason` 으로.**
    ///
    /// 이 줄에서 로케일을 안 타는 것이 무엇인지가 이 파일의 계약이다. 산문은 답한 쪽이
    /// 만들어 보내므로 plugin 이 답하면 앱 언어를 탄다 — 그때도 `code` 는 안 흔들린다.
    #[test]
    fn the_code_is_a_field_not_something_to_parse_out_of_the_prose() {
        let line = format_line(
            "codex.hook",
            &serde_json::json!({ "event": "stop" }),
            Some(-32602),
            "invalid params: 알 수 없는 hook 이벤트",
        );
        assert!(line.contains(" code=-32602 "), "{line}");

        // 산문이 어느 언어든 좌표 넷은 공백 구분으로 그대로 읽힌다.
        let fields: Vec<&str> = line.trim_end().split(' ').take(5).collect();
        assert_eq!(fields[1], "method=codex.hook");
        assert_eq!(fields[2], "event=stop");
        assert_eq!(fields[4], "code=-32602");

        // `reason` 은 마지막이다 — 공백을 담을 수 있는 값이 그것뿐이므로, 읽는 쪽이
        // `reason=` 뒤를 줄 끝까지로 자를 수 있다. 필드를 그 뒤에 더하면 깨진다.
        let at = line.find("reason=").expect("reason 필드");
        assert!(
            !line[at..].trim_end().contains(' ') || line[at..].starts_with("reason="),
            "reason 이 마지막이 아니다: {line}"
        );
        assert!(
            line.trim_end().ends_with("알 수 없는 hook 이벤트"),
            "{line}"
        );
    }

    /// 코드가 없는 실패(호스트에 닿지도 못했다)는 `-` 다 — 있는 척하지 않는다.
    #[test]
    fn a_failure_without_a_code_keeps_the_column() {
        let line = format_line(
            "claude.hook",
            &serde_json::json!({ "event": "stop" }),
            None,
            "no port file",
        );
        assert!(line.contains(" code=- "), "{line}");
    }

    /// 홈 아래 디렉터리가 없어도 만들어 기록한다(첫 실행 환경).
    #[test]
    fn record_creates_missing_parent_dir() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("nested").join(LOG_FILE);
        record_at(
            &path,
            "claude.hook",
            &serde_json::json!({ "event": "session-start" }),
            None,
            "no port file",
        );
        assert!(path.exists(), "부모 디렉터리를 만들어 기록해야 한다");
    }
}
