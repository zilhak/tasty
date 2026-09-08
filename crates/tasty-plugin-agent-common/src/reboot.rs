//! reboot 시퀀스 중 CLI 를 몰라도 성립하는 조각.
//!
//! **여기 없는 것이 더 중요하다.** `EXIT_WAIT` / `RETURN_WAIT`(종료·복귀 대기 한도)는
//! 각 CLI 의 실측치라 plugin 마다 다르고, 지금 값이 같은 `CTRL_C_COUNT` 같은 상수도
//! 원인이 CLI 쪽에 있어 공유하지 않는다. 화면 마커·안내문 키도 마찬가지다.

use std::thread;
use std::time::Duration;

use serde_json::{Value, json};
use tasty_plugin_sdk::HostHandle;

/// 명령 접수 → kill 시작까지 기본 대기 (초). `--delay` 로 오버라이드.
pub const DEFAULT_DELAY_SECS: u64 = 5;

/// 문구 확인 후 추가 Enter 전까지 대기. tell 의 본문/`\r` 분리 write 도 TUI 부팅
/// 직후엔 한 read burst 로 합쳐져 `\r` 이 paste 로 흡수될 수 있다(실측: 문구가
/// 입력창에 미제출로 잔류). 이미 제출된 경우 빈 입력창 Enter 는 no-op 이므로
/// 확인 후 별도 Enter 1회는 항상 안전하다.
const NOTICE_SUBMIT_DELAY: Duration = Duration::from_millis(500);

/// 안내문 = 기본 문구 + (있으면) 빈 줄 하나를 사이에 둔 추가 텍스트.
///
/// **기본 문구를 여기서 짓지 않고 받는다.** 짝의 두 plugin 이 서로 다른 카탈로그
/// 키를 쓰는데, 한쪽 함수가 자기 키를 안에서 찾고 있었다 — 그러면 이 조립 규칙이
/// 그 키에 묶여 공유가 안 된다. 규칙은 "빈 줄 하나" 하나뿐이고 그것만 여기 있다.
pub fn build_notice(base: &str, extra: Option<&str>) -> String {
    match extra {
        Some(t) => format!("{base}\n\n{t}"),
        None => base.to_string(),
    }
}

/// `surface.screen_text` 1 회 조회. 실패 → `None`(surface 소멸 등).
pub fn screen_text<H: crate::host_call::HostCall>(host: &H, surface_id: u32) -> Option<String> {
    host.call("surface.screen_text", json!({ "surface_id": surface_id }))
        .ok()
        .and_then(|r| r.get("text").and_then(|t| t.as_str().map(str::to_string)))
}

/// 지금 화면에 문구가 보이는가. 조회 실패는 **`false`** — 못 봤다는 것을 봤다로
/// 읽으면 확인 없이 다음 단계로 넘어간다.
pub fn screen_contains<H: crate::host_call::HostCall>(
    host: &H,
    surface_id: u32,
    needle: &str,
) -> bool {
    screen_text(host, surface_id)
        .map(|t| t.contains(needle))
        .unwrap_or(false)
}

/// `--delay`(기본 [`DEFAULT_DELAY_SECS`]) / `--prompt`(안내문 뒤에 덧붙일 추가
/// 텍스트) 파싱. 빈 프롬프트는 `None` 으로 접는다 — "안 줬다" 와 같은 뜻이다.
pub fn parse_options(params: &Value) -> (u64, Option<String>) {
    let delay = params
        .get("delay")
        .and_then(|v| v.as_u64())
        .unwrap_or(DEFAULT_DELAY_SECS);
    let extra = params
        .get("prompt")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(String::from);
    (delay, extra)
}

/// session id 가 셸에 평문으로 들어가므로 uuid 계열 문자만 허용한다.
pub fn is_safe_session_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// 안내문이 화면에 뜬 것을 확인한 뒤 Enter 를 한 번 더 보낸다 — [`NOTICE_SUBMIT_DELAY`]
/// 의 이유. `agent` 는 로그 식별용 이름("claude" / "codex")이다.
pub fn ensure_submitted(host: &HostHandle, surface_id: u32, agent: &str) {
    thread::sleep(NOTICE_SUBMIT_DELAY);
    if let Err(e) = host.call(
        "surface.send_key",
        json!({ "surface_id": surface_id, "key": "enter" }),
    ) {
        tracing::warn!("{agent} reboot s{surface_id}: extra submit enter failed: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_defaults_delay_and_no_prompt() {
        let (delay, extra) = parse_options(&json!({ "surface_id": 1 }));
        assert_eq!(delay, DEFAULT_DELAY_SECS);
        assert!(extra.is_none());
    }

    #[test]
    fn parse_explicit_delay_and_prompt() {
        let (delay, extra) = parse_options(&json!({ "delay": 2, "prompt": "빌드부터 다시 확인" }));
        assert_eq!(delay, 2);
        assert_eq!(extra.as_deref(), Some("빌드부터 다시 확인"));
    }

    #[test]
    fn parse_empty_prompt_treated_as_none() {
        let (_, extra) = parse_options(&json!({ "prompt": "" }));
        assert!(extra.is_none());
    }

    #[test]
    fn safe_session_id_accepts_uuid() {
        assert!(is_safe_session_id("0e5cbdf4-32a1-4a5c-9c1d-8f2b3a4c5d6e"));
        assert!(is_safe_session_id("019f55e7-3dfa-7292-a8a9-9cf73a8b000b"));
        assert!(is_safe_session_id("abc_DEF-123"));
    }

    #[test]
    fn safe_session_id_rejects_shell_metachars() {
        assert!(!is_safe_session_id(""));
        assert!(!is_safe_session_id("abc; rm -rf /"));
        assert!(!is_safe_session_id("a b"));
        assert!(!is_safe_session_id("a$(x)"));
        assert!(!is_safe_session_id("a&b"));
    }
}
