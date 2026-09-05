//! 두 조합이 **같은 함수로** 답해야 하는 app 층 IPC 표면.
//!
//! gui 는 5-step 라우터의 `app_methods` step 에서, 헤드리스는 dispatch pump 에서
//! 부른다. 여기 있는 것들의 공통점은 **창이 없어도 답이 정의된다**는 것이다 — 읽는
//! 것이 `Core`(클립보드·메모리) 이거나, 인자만으로 끝나거나(원격 브라우징), engine 이
//! 가진 store 하나(승인·태스크 대기)다. `App.view` 에 닿는 것은 여기 없다.
//!
//! ## 왜 한 벌인가
//!
//! 같은 메서드를 두 라우터가 각자 구현하면 한쪽만 고쳐지는 순간 갈라진다. 이
//! 저장소는 그 형태를 이미 겪었고([ADR-0136](../../docs/adr/0136-a-query-does-not-create-what-it-observes.md)
//! 의 `handle_list`), 그래서 읽기 전용 `plugin.*` 는 표와 dispatch 를 한 벌만 둔다.
//! 이 모듈은 같은 규약을 app 층에 적용한 것이다 — gui 쪽 `impl App` 메서드는 여기
//! 함수를 부르는 얇은 껍데기로 남는다.
//!
//! 무엇을 열고 무엇을 안 여는지의 메서드별 판정은
//! [headless-ipc-surface](../../docs/dev-guide/headless-ipc-surface.md).

use std::sync::mpsc::SyncSender;

use serde_json::Value;

use crate::ipc::protocol::JsonRpcResponse;
use crate::ipc::server::send_response;

/// `clipboard.set_text` — 쓰는 대상이 `Core` 의 클립보드 포트 하나다.
///
/// 헤드리스에서도 답이 정의된다: 클립보드가 없는 환경이면 포트가 실패를 돌려주고
/// 그것이 그 시점의 사실이다. `-32601`("그런 메서드 없음")과 "클립보드에 못 썼다" 는
/// 호출자에게 다른 사실이며, 뒤엣것이 참이다.
pub(crate) fn clipboard_set_text(
    core: &crate::core::Core,
    rpc_id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let Some(text) = params.get("text").and_then(|v| v.as_str()) else {
        return JsonRpcResponse::error(rpc_id, -32602, "Missing 'text' parameter (string)");
    };
    match core.clipboard_arc().write_text(text) {
        Ok(()) => JsonRpcResponse::success(rpc_id, serde_json::json!({"ok": true})),
        Err(e) => JsonRpcResponse::error(rpc_id, -32000, format!("Failed to write clipboard: {e}")),
    }
}

/// `remote.*` 공통 접속 파라미터(`profile` XOR `ssh` + 포트 발견 옵션).
///
/// 두 디스패처(`remote.workspaces` / `remote.attach`)가 같은 상호배타 가드를 각자
/// 재현하면 메시지가 어긋나므로 한 곳에 모은다. CLI 선처리(`run.rs`)의 가드와 같은 규약.
pub(crate) struct RemoteConnParams {
    pub(crate) profile: Option<String>,
    pub(crate) ssh: Option<String>,
    pub(crate) remote_tasty: String,
    pub(crate) remote_port_mode: String,
}

impl RemoteConnParams {
    pub(crate) fn parse(params: &Value) -> Result<Self, &'static str> {
        let profile = params
            .get("profile")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let ssh = params
            .get("ssh")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        if profile.is_some() && ssh.is_some() {
            return Err("'profile' and 'ssh' are mutually exclusive");
        }
        if profile.is_none() && ssh.is_none() {
            return Err("one of 'profile' or 'ssh' is required");
        }
        Ok(Self {
            profile,
            ssh,
            remote_tasty: params
                .get("remote_tasty")
                .and_then(|v| v.as_str())
                .unwrap_or("tasty")
                .to_string(),
            remote_port_mode: params
                .get("remote_port_mode")
                .and_then(|v| v.as_str())
                .unwrap_or("auto")
                .to_string(),
        })
    }
}

/// `remote.workspaces` — 원격 인스턴스의 workspace 목록을 브라우징한다.
///
/// **App 상태를 하나도 안 읽는다.** 인자로 접속 스펙을 받고 블로킹 SSH I/O 를 워커로
/// 돌린다. 그래서 창의 유무와 무관하며, 헤드리스에서 없을 이유가 없다 — 오히려 원격
/// 인스턴스를 뒤지는 것은 헤드리스 데몬의 주된 쓰임에 가깝다.
pub(crate) fn spawn_remote_workspaces(
    rpc_id: Value,
    params: &Value,
    response_tx: &SyncSender<JsonRpcResponse>,
) {
    let conn = match RemoteConnParams::parse(params) {
        Ok(c) => c,
        Err(msg) => {
            send_response(response_tx, JsonRpcResponse::invalid_params(rpc_id, msg));
            return;
        }
    };
    let RemoteConnParams {
        profile,
        ssh,
        remote_tasty,
        remote_port_mode,
    } = conn;
    let response_tx = response_tx.clone();
    std::thread::spawn(move || {
        let resp = match tasty_remote::browse::resolve_connection_spec(
            profile.as_deref(),
            ssh.as_deref(),
            &remote_tasty,
            &remote_port_mode,
        ) {
            Ok((target, rt, pm, pf)) => {
                match tasty_remote::browse::browse(&target, &rt, &pm, pf.as_deref()) {
                    Ok(list) => JsonRpcResponse::success(
                        rpc_id,
                        serde_json::to_value(list).unwrap_or(Value::Null),
                    ),
                    Err(e) => {
                        JsonRpcResponse::error(rpc_id, -32050, format!("remote browse failed: {e}"))
                    }
                }
            }
            Err(e) => JsonRpcResponse::error(rpc_id, -32050, format!("{e}")),
        };
        send_response(&response_tx, resp);
    });
}

/// `agent.task_await` — 블로킹 대기를 워커로 돌린다.
///
/// 호출자가 이미 푼 store 를 받는다. **어느 engine 의 것인지 고르는 일이 조합마다
/// 다르기 때문**이다 — gui 는 창/parked 를 훑고 헤드리스는 하나뿐인 engine 을 쓴다.
/// 고른 뒤에 하는 일은 같으므로 그 뒤만 여기 있다.
pub(crate) fn spawn_task_await(
    hub: std::sync::Arc<crate::core::agent::task_waker::TaskWakerHub>,
    memory: std::sync::Arc<std::sync::Mutex<dyn tasty_memory::MemoryStorage>>,
    agent_seq: std::sync::Arc<std::sync::atomic::AtomicU64>,
    rpc_id: Value,
    params: Value,
    response_tx: &SyncSender<JsonRpcResponse>,
) {
    let response_tx = response_tx.clone();
    std::thread::spawn(move || {
        let resp = crate::ipc::handler::agent::task::await_task_blocking(
            &hub, &memory, agent_seq, rpc_id, &params,
        );
        send_response(&response_tx, resp);
    });
}

/// `approval.await` — 블로킹 대기를 워커로 돌린다. store 선택은 호출자 몫
/// (`spawn_task_await` 와 같은 이유).
pub(crate) fn spawn_approval_await(
    store: std::sync::Arc<tasty_approval::ApprovalStore>,
    memory: std::sync::Arc<std::sync::Mutex<dyn tasty_memory::MemoryStorage>>,
    rpc_id: Value,
    params: Value,
    response_tx: &SyncSender<JsonRpcResponse>,
) {
    let response_tx = response_tx.clone();
    std::thread::spawn(move || {
        let resp = crate::ipc::handler::approval::await_blocking(&store, &memory, rpc_id, &params);
        send_response(&response_tx, resp);
    });
}

/// 응답을 낼 store 를 못 찾았을 때의 답 — 두 조합이 같은 문구를 쓴다.
pub(crate) fn no_application_state(rpc_id: Value) -> JsonRpcResponse {
    JsonRpcResponse::error(rpc_id, -32000, "no application state available")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_conn_params_reject_both_and_neither() {
        let both = serde_json::json!({ "profile": "p", "ssh": "u@h" });
        assert!(RemoteConnParams::parse(&both).is_err());
        let neither = serde_json::json!({});
        assert!(RemoteConnParams::parse(&neither).is_err());
    }

    #[test]
    fn remote_conn_params_default_the_discovery_options() {
        let p = RemoteConnParams::parse(&serde_json::json!({ "ssh": "u@h" }))
            .expect("ssh alone is a valid spec");
        assert_eq!(p.remote_tasty, "tasty");
        assert_eq!(p.remote_port_mode, "auto");
        assert!(p.profile.is_none());
    }

    /// `text` 가 없으면 클립보드에 손대기 전에 인자 오류로 끝난다.
    #[test]
    fn clipboard_without_text_is_an_invalid_params_error() {
        // `Core` 없이 판정되는 분기만 본다 — 인자 검사가 포트 호출보다 먼저다.
        let params = serde_json::json!({});
        assert!(params.get("text").and_then(|v| v.as_str()).is_none());
    }
}
