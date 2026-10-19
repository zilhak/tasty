//! `run.result`: 접수 응답의 `run`으로 그 요청의 진행·최종 결과를 조회한다([`crate::request_runs`]).

use tasty_ipc::protocol::JsonRpcResponse;

pub(super) fn handle(
    id: serde_json::Value,
    caller: &tasty_ipc::caller::CallerContext,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let Some(run) = params.get("run").and_then(|v| v.as_u64()) else {
        return JsonRpcResponse::invalid_params(id, "Missing required 'run' parameter");
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
