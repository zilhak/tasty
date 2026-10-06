//! agent task 의 턴 보고. provider 훅이 턴의 시작과 끝을 호스트(`agent.task_turn_report`)에
//! 알린다. 호스트는 surface 에 묶인 agent task 회차가 있을 때만 적용하고, 없으면 무시한다.
//! 결과를 확정하거나 task 를 끝내는 것은 호스트다.

use serde_json::{Value, json};

use crate::host_call::HostCall;

/// 훅 이벤트가 뜻하는 턴 보고.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnReport<'a> {
    Started,
    Ended { final_answer: Option<&'a str> },
    Failed { error: &'a str },
}

/// 보고 params.
pub fn report_params(provider: &str, surface_id: u32, report: &TurnReport<'_>) -> Value {
    match report {
        TurnReport::Started => json!({
            "provider": provider,
            "surface_id": surface_id,
            "event": "turn_started",
        }),
        TurnReport::Ended { final_answer } => json!({
            "provider": provider,
            "surface_id": surface_id,
            "event": "turn_ended",
            "final_answer": final_answer,
        }),
        TurnReport::Failed { error } => json!({
            "provider": provider,
            "surface_id": surface_id,
            "event": "turn_ended",
            "error": error,
        }),
    }
}

/// 보고를 보낸다. 실패는 로그만 남긴다(훅의 다른 처리를 막지 않는다).
pub fn send<H: HostCall>(host: &H, provider: &str, surface_id: u32, report: &TurnReport<'_>) {
    if let Err(e) = host.call(
        "agent.task_turn_report",
        report_params(provider, surface_id, report),
    ) {
        tracing::warn!("{provider} hook s{surface_id}: agent.task_turn_report failed: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_end_carries_the_final_answer_and_a_failure_carries_the_error() {
        let p = report_params(
            "claude",
            3,
            &TurnReport::Ended {
                final_answer: Some("done"),
            },
        );
        assert_eq!(p["event"], "turn_ended");
        assert_eq!(p["final_answer"], "done");
        assert!(p.get("error").is_none());
        let p = report_params(
            "codex",
            3,
            &TurnReport::Failed {
                error: "overloaded",
            },
        );
        assert_eq!(p["error"], "overloaded");
        assert_eq!(
            report_params("claude", 3, &TurnReport::Started)["event"],
            "turn_started"
        );
    }
}
