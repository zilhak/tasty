//! 요청이 상대에게 **요구하는 계약**을 보내기 전에 확인한다.
//!
//! 봉투의 새 필드와 메서드의 새 인자는 그것을 모르는 서버에서 **조용히 버려진다** — 봉투에도
//! 인자 객체에도 모르는 키 거절이 없다. 버려진 요청은 성공으로 돌아오므로 호출자는 계약이
//! 걸린 줄 안다. 그래서 그런 필드·인자를 실은 요청은 `system.info` 의 capability 목록에서
//! 그 이름을 먼저 묻고, 없으면 **요청을 내보내지 않는다.**
//!
//! 무엇을 요구하는지는 명령이 아니라 **요청 자체**에서 판정한다. 같은 판정을 정적 CLI 와
//! plugin CLI 가 함께 쓰려면 둘이 공유하는 것이 요청뿐이다.
//!
//! 근거: `docs/adr/0365-the-output-cursor-contract-is-negotiated-by-name-before-the-cli-sends-it.md`.

use anyhow::Result;
use tasty_ipc::capability::{RESPONSE_TIMEOUT, RESPONSE_TIMEOUT_VERSION};
use tasty_ipc::client::{IpcConnection, UnsupportedCapability};
use tasty_ipc::output_cursor;
use tasty_ipc::protocol::JsonRpcRequest;

/// 이 요청이 상대에게 요구하는 계약 — `(이름, 최소 판)`.
///
/// 비어 있으면 묻지 않는다. 새 계약을 하나도 안 쓰는 요청은 구 서버에서도 뜻이 같으므로,
/// 물으면 기존 호출에 왕복 하나를 얹을 뿐이다.
pub(crate) fn required(request: &JsonRpcRequest) -> Vec<(&'static str, u32)> {
    let mut out = Vec::new();
    // `Some(0)` 은 봉투 규약상 "상한 없음" 이라 계약을 안 쓴다.
    if request.response_timeout_ms.is_some_and(|ms| ms > 0) {
        out.push((RESPONSE_TIMEOUT, RESPONSE_TIMEOUT_VERSION));
    }
    if output_cursor::requested_by(&request.method, &request.params) {
        out.push((output_cursor::CAPABILITY, output_cursor::VERSION));
    }
    out
}

/// 요구하는 계약을 상대가 선언했는지 확인한다. 하나라도 없으면 [`UnsupportedCapability`]
/// 를 담은 오류로 끝나고, 그때 **요청은 아직 안 나갔다.**
///
/// 확인 자체(`system.info`)가 실패하면 그 오류가 그대로 온다 — 그것은 거절이 아니라 연결의
/// 실패다.
pub(crate) fn ensure(conn: &mut IpcConnection, request: &JsonRpcRequest) -> Result<()> {
    for (name, version) in required(request) {
        conn.require_capability(name, version, request.session_token.as_deref())?;
    }
    Ok(())
}

/// 거절을 stderr 에 낼 **한 줄 JSON**. 사람이 읽는 문장은 `message` 에 싣고, 호출자가
/// 분기할 값(`kind` · `capability` · `required` · `found` · `sent`)은 따로 싣는다 — 문장을
/// 파싱하게 두면 그 판정이 로케일에 묶인다.
pub(crate) fn refusal_line(refusal: &UnsupportedCapability) -> String {
    let message = match refusal.found {
        Some(found) => tasty_i18n::t_args(
            "cli.contract.unsupported_older",
            &[
                &refusal.name,
                &found.to_string(),
                &refusal.required.to_string(),
            ],
        ),
        None => tasty_i18n::t_args(
            "cli.contract.unsupported_missing",
            &[&refusal.name, &refusal.required.to_string()],
        ),
    };
    serde_json::json!({
        "error": {
            "kind": "unsupported_capability",
            "capability": refusal.name,
            "required": refusal.required,
            "found": refusal.found,
            "sent": false,
            "message": message,
        }
    })
    .to_string()
}

/// [`ensure`] 의 오류를 사용자에게 내고 종료한다. 거절이면 구조화된 한 줄, 그 밖의 실패
/// (확인 요청이 안 닿음 등)는 다른 전송 실패와 같은 모양이다. 종료 코드는 둘 다 1 이다.
pub(crate) fn exit_on_failure(e: anyhow::Error) -> ! {
    match e.downcast_ref::<UnsupportedCapability>() {
        Some(refusal) => eprintln!("{}", refusal_line(refusal)),
        None => eprintln!("{e}"),
    }
    std::process::exit(1);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn req(method: &str, params: serde_json::Value, timeout: Option<u64>) -> JsonRpcRequest {
        JsonRpcRequest {
            jsonrpc: "2.0".into(),
            method: method.into(),
            params,
            id: Some(json!(1)),
            session_token: None,
            response_timeout_ms: timeout,
            idempotency_key: None,
        }
    }

    #[test]
    fn a_request_that_uses_no_new_contract_asks_nothing() {
        let r = req(
            "surface.read_since_mark",
            json!({ "surface_id": 1, "strip_ansi": true }),
            None,
        );
        assert!(required(&r).is_empty());
        // `Some(0)` 은 상한 없음 — 계약을 안 쓴다.
        let r = req("workspace.list", json!({}), Some(0));
        assert!(required(&r).is_empty());
    }

    #[test]
    fn a_positive_bound_requires_the_response_timeout_contract() {
        let r = req("workspace.list", json!({}), Some(500));
        assert_eq!(
            required(&r),
            vec![(RESPONSE_TIMEOUT, RESPONSE_TIMEOUT_VERSION)]
        );
    }

    #[test]
    fn a_position_argument_requires_the_output_cursor_contract() {
        let r = req(
            "surface.read_since_mark",
            json!({ "surface_id": 1, "max_bytes": 64 }),
            Some(10),
        );
        assert_eq!(
            required(&r),
            vec![
                (RESPONSE_TIMEOUT, RESPONSE_TIMEOUT_VERSION),
                (output_cursor::CAPABILITY, output_cursor::VERSION),
            ]
        );
    }

    fn since_mark(args: &[&str]) -> JsonRpcRequest {
        use clap::Parser;
        let mut argv = vec!["tasty", "read", "since-mark", "--surface", "3"];
        argv.extend_from_slice(args);
        let cli = crate::Cli::try_parse_from(argv).expect("파싱");
        crate::request::command_to_request(&cli.command.expect("명령"))
    }

    /// 위치 인자를 안 준 호출은 **예전과 같은 요청**이다 — 인자 키가 아예 없고, 그래서
    /// 구 서버에 계약 확인도 안 붙는다.
    #[test]
    fn a_plain_since_mark_sends_the_same_params_as_before_and_asks_nothing() {
        let r = since_mark(&["--strip-ansi"]);
        assert_eq!(r.method, "surface.read_since_mark");
        assert_eq!(r.params, json!({ "surface_id": 3, "strip_ansi": true }));
        assert!(required(&r).is_empty());
    }

    #[test]
    fn position_arguments_are_carried_and_require_the_contract() {
        let r = since_mark(&["--cursor", "120", "--stream", "s1", "--max-bytes", "64"]);
        assert_eq!(r.params["cursor"], 120);
        assert_eq!(r.params["stream"], "s1");
        assert_eq!(r.params["max_bytes"], 64);
        assert_eq!(
            required(&r),
            vec![(output_cursor::CAPABILITY, output_cursor::VERSION)]
        );
    }

    /// `--cursor` 만 주면 통신 전에 clap 이 막는다 — 서버가 `cursor_without_stream` 으로
    /// 거절할 조합이다. `--stream` 단독은 서버가 받는 형태라 허용한다.
    #[test]
    fn a_cursor_without_a_stream_is_refused_before_anything_is_sent() {
        use clap::Parser;
        assert!(
            crate::Cli::try_parse_from(["tasty", "read", "since-mark", "--cursor", "1"]).is_err()
        );
        let r = since_mark(&["--stream", "s1"]);
        assert_eq!(r.params["stream"], "s1");
        assert!(r.params.get("cursor").is_none());
    }

    /// 거절 줄은 **한 줄 JSON** 이고, 호출자가 분기할 값을 문장과 따로 싣는다. 특히
    /// `sent:false` — 호스트가 답한 실패와 이 거절을 가르는 것이 그 값이다.
    #[test]
    fn the_refusal_is_one_json_line_that_says_nothing_was_sent() {
        let missing = UnsupportedCapability {
            name: "ipc.output-cursor".into(),
            required: 1,
            found: None,
        };
        let line = refusal_line(&missing);
        assert!(!line.contains('\n'));
        let v: serde_json::Value = serde_json::from_str(&line).expect("JSON");
        let e = &v["error"];
        assert_eq!(e["kind"], "unsupported_capability");
        assert_eq!(e["capability"], "ipc.output-cursor");
        assert_eq!(e["required"], 1);
        assert!(e["found"].is_null(), "없는 것과 낮은 판은 다른 값이다");
        assert_eq!(e["sent"], false);
        assert!(e["message"].is_string());

        let older = UnsupportedCapability {
            name: "x".into(),
            required: 2,
            found: Some(1),
        };
        let v: serde_json::Value = serde_json::from_str(&refusal_line(&older)).expect("JSON");
        assert_eq!(v["error"]["found"], 1);
    }
}
