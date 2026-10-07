//! Claude 훅을 agent task 턴 보고로 바꾼다. 턴을 끝내지 않는 Stop(백그라운드 대기·게이트
//! 보류)은 호출 전에 걸러진다. 게이트가 보류한 Stop 은 판정이 턴 종료로 확정될 때
//! [`report_ended`] 로 보고한다.

use serde_json::Value;
use tasty_plugin_agent_common::host_call::HostCall;
use tasty_plugin_agent_common::task_turn::{TurnReport, send};

/// 턴의 시작·끝에 해당하는 이벤트를 호스트에 보고한다.
pub(crate) fn report<H: HostCall>(host: &H, event: &str, surface_id: u32, params: &Value) {
    if let Some(r) = turn_report(event, params) {
        send(host, "claude", surface_id, &r);
    }
}

/// 게이트 판정으로 턴 종료가 확정된 Stop 을 보고한다. 보류할 때 보관한 최종 답변을 싣는다.
pub(crate) fn report_ended<H: HostCall>(host: &H, surface_id: u32, final_answer: Option<&str>) {
    send(
        host,
        "claude",
        surface_id,
        &TurnReport::Ended { final_answer },
    );
}

/// Stop 의 최종 답변. 게이트가 보류한 Stop 은 확정될 때까지 이 값을 보관한다.
pub(crate) fn final_answer(params: &Value) -> Option<String> {
    params
        .get("last_assistant_message")
        .and_then(Value::as_str)
        .map(String::from)
}

fn turn_report<'a>(event: &str, params: &'a Value) -> Option<TurnReport<'a>> {
    let text = |k: &str| params.get(k).and_then(|v| v.as_str());
    match event {
        "prompt-submit" => Some(TurnReport::Started {
            prompt: text("prompt"),
        }),
        "stop" => Some(TurnReport::Ended {
            final_answer: text("last_assistant_message"),
        }),
        "stop-failure" => Some(TurnReport::Failed {
            error: text("error").filter(|e| !e.is_empty()).unwrap_or("unknown"),
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn only_turn_boundaries_become_reports() {
        let p = json!({ "last_assistant_message": "answer", "error": "rate_limit" });
        assert_eq!(
            turn_report("prompt-submit", &p),
            Some(TurnReport::Started { prompt: None })
        );
        assert_eq!(
            turn_report("prompt-submit", &json!({ "prompt": "hi" })),
            Some(TurnReport::Started { prompt: Some("hi") })
        );
        assert_eq!(
            turn_report("stop", &p),
            Some(TurnReport::Ended {
                final_answer: Some("answer")
            })
        );
        assert_eq!(
            turn_report("stop-failure", &p),
            Some(TurnReport::Failed {
                error: "rate_limit"
            })
        );
        assert_eq!(
            turn_report("stop-failure", &json!({})),
            Some(TurnReport::Failed { error: "unknown" })
        );
        for other in [
            "notification",
            "session-end",
            "pre-tool-use",
            "subagent-stop",
        ] {
            assert_eq!(turn_report(other, &p), None, "{other}");
        }
    }
}
