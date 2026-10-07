//! Claude 훅을 agent task 턴 보고로 바꾼다. 턴을 끝내지 않는 Stop(백그라운드 대기·게이트
//! 보류)은 호출 전에 걸러진다.

use serde_json::Value;
use tasty_plugin_agent_common::host_call::HostCall;
use tasty_plugin_agent_common::task_turn::{TurnReport, send};

/// 턴의 시작·끝에 해당하는 이벤트를 호스트에 보고한다.
pub(crate) fn report<H: HostCall>(host: &H, event: &str, surface_id: u32, params: &Value) {
    if let Some(r) = turn_report(event, params) {
        send(host, "claude", surface_id, &r);
    }
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
