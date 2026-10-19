//! `run.result`: 접수 응답의 `run`으로 그 요청의 진행·최종 결과를 조회한다([`crate::request_runs`]).

use tasty_ipc::protocol::JsonRpcResponse;

pub(super) fn handle(
    id: serde_json::Value,
    caller: &tasty_ipc::caller::CallerContext,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let run = match super::params::opt_int::<u64>(params, "run", &id) {
        Ok(Some(run)) => run,
        Ok(None) => {
            return JsonRpcResponse::invalid_params(id, "Missing required 'run' parameter");
        }
        Err(response) => return response,
    };
    let reader = crate::request_runs::reader_of(caller);
    match crate::request_runs::lookup(run, reader.as_deref()) {
        Some(result) => JsonRpcResponse::success(id, result),
        None => JsonRpcResponse::invalid_params(
            id,
            format!(
                "unknown run {run} — not issued by this caller, or older than the last {} runs",
                crate::request_runs::KEEP
            ),
        ),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    fn call(params: serde_json::Value) -> tasty_ipc::protocol::JsonRpcResponse {
        super::handle(
            json!(1),
            &tasty_ipc::caller::CallerContext::local_claiming(None),
            &params,
        )
    }

    #[test]
    fn run_is_read_through_the_shared_number_parser() {
        let missing = call(json!({})).error.unwrap();
        assert_eq!(missing.code, -32602);
        assert_eq!(missing.message, "Missing required 'run' parameter");

        for bad in [json!(-1), json!("3"), json!(1.5)] {
            let error = call(json!({ "run": bad })).error.unwrap();
            assert_eq!(error.code, -32602);
            assert!(
                error.message.starts_with("'run' was given as"),
                "{}",
                error.message
            );
        }

        let run = crate::request_runs::begin("run.result", None, serde_json::Value::Null);
        let found = call(json!({ "run": run })).result.unwrap();
        assert_eq!(found["state"], "running");
    }
}
