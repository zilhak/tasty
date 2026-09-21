//! `surface.read_since_mark` 의 **소비자 보유 위치** 계약 — 인자 이름과 그 계약의 판.
//!
//! 이 인자들(`cursor` · `stream` · `max_bytes`)을 모르는 구 서버는 그것을 **조용히
//! 버리고** 서버 마크에서 읽는다(인자 객체에 모르는 키 거절이 없다). 응답은 성공이고
//! `text` 도 있으므로 호출자는 자기 위치가 적용됐다고 믿지만, 실제로는 공유 마크를
//! 읽었고 `next_cursor` 도 없다. 그래서 이 인자를 싣는 client 는 **보내기 전에**
//! `system.info` 의 capability 목록에서 [`CAPABILITY`] 를 물어야 한다
//! ([`crate::client::IpcConnection::require_capability`]).
//!
//! 이름과 판을 한 모듈에 두는 이유는 판이 **배선과 같은 자리에서 나오게** 하려는 것이다.
//! 서버의 인자 파서(`surface.read_since_mark` 핸들러)가 이 모듈의 이름으로 인자를 읽고,
//! [`crate::capability::CAPABILITIES`] 가 이 모듈의 [`VERSION`] 을 싣고, CLI 가 이 모듈의
//! 이름으로 인자를 싣는다. 셋이 각자 리터럴을 적으면 셋이 갈린다 — `ipc.stream` 이
//! [`crate::stream::STREAM_PROTO`] 를 그대로 싣는 것과 같은 모양이다.
//!
//! 계약 본문(위치의 단위 · gap · 다른 스트림 거절)은
//! `docs/adr/0341-a-terminal-output-read-answers-from-a-position-the-consumer-holds.md`,
//! 이 이름을 협상 목록에 올린 결정은
//! `docs/adr/0365-the-output-cursor-contract-is-negotiated-by-name-before-the-cli-sends-it.md`.

/// 협상 목록에 오르는 이름.
pub const CAPABILITY: &str = "ipc.output-cursor";

/// 이 계약의 판. 인자의 **뜻**이 좁아질 때만 올린다 — 인자를 더하는 것은 판이 아니라
/// 새 이름이다(구 client 가 모르는 인자는 안 싣고, 구 서버는 모르는 인자를 버리므로).
pub const VERSION: u32 = 1;

/// 호출자가 든 위치(원문 바이트 단위, 절대값).
pub const PARAM_CURSOR: &str = "cursor";
/// 그 위치가 속한 스트림의 표지. `cursor` 에는 이것이 따라야 한다.
pub const PARAM_STREAM: &str = "stream";
/// 이번 읽기가 나를 원문 바이트 상한.
pub const PARAM_MAX_BYTES: &str = "max_bytes";

/// 이 계약에 속하는 인자 전부. 하나라도 실리면 그 요청은 계약을 요구한다.
pub const PARAMS: [&str; 3] = [PARAM_CURSOR, PARAM_STREAM, PARAM_MAX_BYTES];

/// 이 계약을 쓰는 메서드.
pub const METHOD: &str = "surface.read_since_mark";

/// 이 요청이 이 계약을 **요구하는가** — 메서드가 맞고 계약의 인자가 하나라도 실렸는가.
///
/// 인자가 하나도 없는 요청은 종전 그대로 서버 마크를 읽으므로 구 서버에서도 뜻이 같다.
/// 그 요청까지 막으면 구 서버에 대한 기존 호출이 깨진다.
pub fn requested_by(method: &str, params: &serde_json::Value) -> bool {
    method == METHOD
        && PARAMS
            .iter()
            .any(|k| params.get(k).is_some_and(|v| !v.is_null()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_read_without_the_contract_s_arguments_does_not_require_it() {
        assert!(!requested_by(METHOD, &json!({ "surface_id": 1 })));
        assert!(!requested_by(
            METHOD,
            &json!({ "surface_id": 1, "strip_ansi": true, "cursor": null })
        ));
    }

    #[test]
    fn any_one_of_the_three_arguments_requires_it() {
        for k in PARAMS {
            assert!(requested_by(METHOD, &json!({ k: 1 })), "{k}");
        }
    }

    #[test]
    fn another_method_carrying_the_same_names_does_not_require_it() {
        // `events.fetch` 도 위치를 받지만 다른 계약이다.
        assert!(!requested_by("events.fetch", &json!({ "cursor": 1 })));
    }
}
