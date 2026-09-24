//! 호스트 오류를 `Error (<code>): <message>`로 출력한다.
//! error.data가 있으면 다음 줄에 `data: <한 줄 JSON>`을 붙인다. 두 줄의 형식은 파서 계약이다.

use tasty_ipc::client::JsonRpcCallError;

/// 둘째 줄의 접두. 값 자리는 로케일을 안 타는 JSON 이라 이 접두도 번역하지 않는다 —
/// 첫 줄의 `Error (` 와 같은 응답 형식의 일부다.
pub(crate) const DATA_LINE_PREFIX: &str = "data: ";

/// stderr 에 낼 줄들. JSON-RPC 오류가 아니면 종전처럼 `Display` 한 줄이다.
pub(crate) fn render(e: &anyhow::Error) -> String {
    let Some(rpc) = e.downcast_ref::<JsonRpcCallError>() else {
        return e.to_string();
    };
    match rpc.data.as_ref().filter(|d| !d.is_null()) {
        Some(data) => format!(
            "{rpc}\n{DATA_LINE_PREFIX}{}",
            // compact 직렬화는 중첩 값에도 개행을 넣지 않는다 — 둘째 줄은 정확히 한 줄이다.
            serde_json::to_string(data).unwrap_or_default()
        ),
        None => rpc.to_string(),
    }
}

/// 오류를 stderr 에 내고 종료 코드 1 로 끝낸다.
pub(crate) fn exit_with(e: &anyhow::Error) -> ! {
    crate::out::errln!("{}", render(e));
    std::process::exit(1);
}

/// main으로 반환하는 오류는 std가 Error:를 추가하므로 그 접두를 유지한다.
/// data가 있으면 render의 두 줄 형식으로 바꾸고, 없으면 원래 오류를 반환한다.
pub(crate) fn with_data_line(e: anyhow::Error) -> anyhow::Error {
    let has_data = e
        .downcast_ref::<JsonRpcCallError>()
        .is_some_and(|rpc| rpc.data.as_ref().is_some_and(|d| !d.is_null()));
    if has_data {
        // 원인 체인(`Caused by:`)을 달지 않는다 — std 의 출력에 그 블록이 붙어 두 줄 모양이 깨진다.
        anyhow::anyhow!("{}", render(&e))
    } else {
        e
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn call_error(data: Option<serde_json::Value>) -> anyhow::Error {
        JsonRpcCallError {
            code: -32603,
            message: "memory store write failed".into(),
            data,
        }
        .into()
    }

    #[test]
    fn data_rides_on_a_second_line_as_compact_json() {
        let e = call_error(Some(json!({
            "storage_failure": "read_only",
            "detail": { "path": "/x", "nested": [1, 2] }
        })));
        let rendered = render(&e);
        let lines: Vec<&str> = rendered.lines().collect();
        assert_eq!(lines.len(), 2, "{rendered}");
        assert_eq!(lines[0], "Error (-32603): memory store write failed");
        let data: serde_json::Value = serde_json::from_str(
            lines[1]
                .strip_prefix(DATA_LINE_PREFIX)
                .expect("second line starts with the data prefix"),
        )
        .expect("second line is JSON");
        assert_eq!(data["storage_failure"], "read_only");
        assert_eq!(data["detail"]["nested"], json!([1, 2]));
    }

    #[test]
    fn without_data_the_line_is_unchanged() {
        assert_eq!(
            render(&call_error(None)),
            "Error (-32603): memory store write failed"
        );
        assert_eq!(
            render(&call_error(Some(serde_json::Value::Null))),
            "Error (-32603): memory store write failed"
        );
    }

    #[test]
    fn the_main_path_keeps_its_first_line_and_adds_the_data_line() {
        // `RUST_BACKTRACE` 가 켜진 환경이면 anyhow 가 뒤에 backtrace 블록을 붙인다 — 그 앞까지만 본다.
        let termination = |e: anyhow::Error| {
            let full = format!("Error: {:?}", with_data_line(e));
            match full.split_once("\n\nStack backtrace:") {
                Some((head, _)) => head.to_string(),
                None => full,
            }
        };
        assert_eq!(
            termination(call_error(Some(json!({ "reason": "evicted" })))),
            "Error: Error (-32603): memory store write failed\ndata: {\"reason\":\"evicted\"}"
        );
        assert_eq!(
            termination(call_error(None)),
            "Error: Error (-32603): memory store write failed"
        );
        assert_eq!(
            termination(call_error(Some(serde_json::Value::Null))),
            "Error: Error (-32603): memory store write failed"
        );
        let other = anyhow::anyhow!("tasty instance closed the connection");
        assert_eq!(
            termination(other),
            "Error: tasty instance closed the connection"
        );
    }

    #[test]
    fn other_errors_keep_their_display() {
        let e = anyhow::anyhow!("tasty instance closed the connection");
        assert_eq!(render(&e), "tasty instance closed the connection");
    }
}
