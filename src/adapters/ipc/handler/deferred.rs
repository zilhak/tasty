//! memory writer의 결과를 기다려 응답하는 요청.
//!
//! 핸들러는 요청을 writer에 넣고 결과로 응답을 만드는 continuation을 등록한 뒤 자리표시 응답을
//! 돌려준다. `handle_checked_request`가 자리표시를 [`Handled::Later`]로 바꾸고, 호출 경로가 원
//! 호출자(IPC 연결·plugin 호출)를 붙여 [`MemoryReplies`]에 넘긴다. 메인 루프가 회차마다 결과를
//! 거두면 continuation이 응답을 만들고 멱등 키 기록을 끝낸 뒤 원 호출자에게 보낸다.
//!
//! - 응답 시점은 commit 뒤다. 기존 IPC의 확정 성공·version·CAS 의미가 유지된다.
//! - 호출자의 응답 시한·연결 종료는 이미 접수한 요청을 취소하지 않는다. 응답만 버려진다.
//! - writer 없는 시험 구성은 결과가 바로 있어 지연하지 않고 그 자리에서 응답한다.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::mpsc::SyncSender;

use serde_json::{Value, json};
use tasty_ipc::protocol::JsonRpcResponse;
use tasty_memory::MemoryError;
use tasty_memory::writer::{Failure, MemoryCompletion, MemoryTicket};
use tasty_utils::batch_writer::SubmitError;

use crate::app::services::AppServices;

/// writer 결과로 응답을 만드는 continuation.
pub(crate) type Finish = Box<dyn FnOnce(&AppServices, MemoryCompletion) -> JsonRpcResponse + Send>;

/// writer에 넣은 요청과 그 결과를 응답으로 바꾸는 continuation.
pub(crate) struct Deferred {
    id: Value,
    ticket: MemoryTicket,
    finish: Finish,
    idempotency: Option<super::idempotency::Pending>,
}

/// 검사한 요청을 실행한 결과. `Later`는 원 호출자를 붙여 [`MemoryReplies::push`]에 넘긴다.
pub(crate) enum Handled {
    Now(JsonRpcResponse),
    Later(Deferred),
}

impl Handled {
    /// 결과를 기다려 응답을 얻는다. 시험과 결과를 기다려도 되는 호출부만 쓴다.
    #[cfg(test)]
    pub(crate) fn wait(self, core: &AppServices) -> JsonRpcResponse {
        match self {
            Self::Now(response) => response,
            Self::Later(mut deferred) => {
                let completion = deferred
                    .ticket
                    .wait_timeout(std::time::Duration::from_secs(30))
                    .expect("memory writer result");
                deferred.complete(core, completion)
            }
        }
    }
}

impl Deferred {
    pub(super) fn with_idempotency(mut self, pending: Option<super::idempotency::Pending>) -> Self {
        self.idempotency = pending;
        self
    }

    fn complete(self, core: &AppServices, completion: MemoryCompletion) -> JsonRpcResponse {
        let response = (self.finish)(core, completion);
        super::idempotency::finish(core.now_instant(), self.idempotency, &response);
        response
    }

    pub(crate) fn id(&self) -> &Value {
        &self.id
    }
}

/// 응답을 받을 원 호출자.
pub(crate) enum ReplyTo {
    Ipc(SyncSender<JsonRpcResponse>),
    /// plugin의 IPC 호출. 응답 때 같은 프로세스인지 binding으로 확인한다.
    Plugin {
        plugin_id: String,
        call_id: u64,
        binding: std::sync::Weak<()>,
    },
}

impl ReplyTo {
    pub(crate) fn plugin(call: &tasty_host_plugin::manager::PendingPluginCall) -> Self {
        Self::Plugin {
            plugin_id: call.plugin_id.clone(),
            call_id: call.call_id,
            binding: call.binding.clone(),
        }
    }
}

/// 끝난 plugin 호출의 응답. plugin 매니저를 가진 루프가 보낸다.
pub(crate) struct PluginReply {
    pub(crate) plugin_id: String,
    pub(crate) call_id: u64,
    pub(crate) binding: std::sync::Weak<()>,
    pub(crate) response: JsonRpcResponse,
}

struct PendingReply {
    deferred: Deferred,
    to: ReplyTo,
}

static SLOT_POISONED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 응답을 기다리는 요청 목록. 개수는 writer 예산이 묶는다. 메인 루프만 쓰지만 `AppServices`가
/// 스레드 사이에서 공유되는 타입이라 잠금 안에 둔다.
#[derive(Default)]
pub(crate) struct MemoryReplies {
    /// 핸들러가 이번 요청에 등록한 continuation. `handle_checked_request`가 바로 꺼낸다.
    slot: Mutex<Option<Deferred>>,
    pending: Mutex<VecDeque<PendingReply>>,
}

impl MemoryReplies {
    fn slot(&self) -> std::sync::MutexGuard<'_, Option<Deferred>> {
        crate::poison::recover_mutex(self.slot.lock(), "memory reply slot", &SLOT_POISONED)
    }

    fn pending(&self) -> std::sync::MutexGuard<'_, VecDeque<PendingReply>> {
        crate::poison::recover_mutex(self.pending.lock(), "memory replies", &SLOT_POISONED)
    }

    pub(super) fn park(&self, deferred: Deferred) {
        if let Some(previous) = self.slot().replace(deferred) {
            tracing::error!(
                id = %previous.id,
                "a handler deferred twice; the earlier result is answered by nobody"
            );
        }
    }

    pub(super) fn take_parked(&self) -> Option<Deferred> {
        self.slot().take()
    }

    pub(crate) fn push(&self, deferred: Deferred, to: ReplyTo) {
        self.pending().push_back(PendingReply { deferred, to });
    }

    /// 끝난 요청의 응답을 만든다. IPC 응답은 바로 보내고 plugin 응답은 돌려준다.
    pub(crate) fn poll(core: &AppServices) -> Vec<PluginReply> {
        let mut ready = Vec::new();
        {
            let mut pending = core.memory_replies().pending();
            let mut index = 0;
            while index < pending.len() {
                match pending[index].deferred.ticket.try_take() {
                    Some(completion) => {
                        if let Some(entry) = pending.remove(index) {
                            ready.push((entry, completion));
                        }
                    }
                    None => index += 1,
                }
            }
        }
        let mut plugin = Vec::new();
        for (entry, completion) in ready {
            let response = entry.deferred.complete(core, completion);
            match entry.to {
                ReplyTo::Ipc(tx) => tasty_ipc::server::send_response(&tx, response),
                ReplyTo::Plugin {
                    plugin_id,
                    call_id,
                    binding,
                } => plugin.push(PluginReply {
                    plugin_id,
                    call_id,
                    binding,
                    response,
                }),
            }
        }
        plugin
    }
}

/// 예산에 셀 요청 크기. 요청 인자의 JSON 길이로 잡는다.
pub(crate) fn params_bytes(params: &Value) -> usize {
    struct Counter(usize);
    impl std::io::Write for Counter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0 += buf.len();
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut counter = Counter(0);
    if serde_json::to_writer(&mut counter, params).is_err() {
        tracing::debug!("request size could not be measured");
    }
    counter.0
}

/// 자리표시 응답. `handle_checked_request` 밖으로 나가면 호출자는 오류를 받는다.
fn placeholder(id: Value) -> JsonRpcResponse {
    JsonRpcResponse::internal_error(id, "memory response was deferred outside a reply path")
}

/// writer가 접수하지 않은 요청의 응답. 포화는 다시 시도할 수 있는 busy로 분류한다.
pub(crate) fn submit_error_response(id: Value, error: &SubmitError) -> JsonRpcResponse {
    let (failure, writer) = match error {
        SubmitError::Saturated { .. } => ("busy", "saturated"),
        SubmitError::Closed => ("other", "closed"),
    };
    JsonRpcResponse::error_with_data(
        id,
        -32603,
        format!("memory {error}"),
        json!({ "storage_failure": failure, "writer": writer }),
    )
}

/// 요청 자신의 오류도 저장 실패도 아닌 결과(확정 여부 불명, 본문 panic)의 응답.
/// `map_error`는 요청이 받은 `MemoryError`를 응답으로 바꾸는 핸들러의 매핑이다.
pub(crate) fn unsettled_response(
    id: Value,
    failure: &Failure,
    map_error: impl FnOnce(Value, &MemoryError) -> JsonRpcResponse,
) -> JsonRpcResponse {
    match failure {
        Failure::Rejected { error, .. } | Failure::Storage(error) => map_error(id, error),
        Failure::Unknown(Some(error)) => {
            let mut response = map_error(id, error);
            if let Some(e) = response.error.as_mut() {
                let data = e.data.get_or_insert_with(|| json!({}));
                if let Some(obj) = data.as_object_mut() {
                    obj.insert("outcome".into(), json!("unknown"));
                }
            }
            response
        }
        Failure::Unknown(None) => JsonRpcResponse::error_with_data(
            id,
            -32603,
            "memory writer ended before the request finished; it may have been stored",
            json!({ "storage_failure": "other", "outcome": "unknown" }),
        ),
        Failure::Panicked => JsonRpcResponse::error_with_data(
            id,
            -32603,
            "memory request panicked",
            json!({ "storage_failure": "other" }),
        ),
    }
}

impl AppServices {
    /// 요청을 writer에 넣고 결과가 나오면 `finish`로 응답한다. 결과가 바로 있으면(시험 구성)
    /// 지금 응답하고, 아니면 continuation을 등록하고 자리표시를 돌려준다.
    pub(crate) fn reply_with_memory(
        &self,
        id: Value,
        request: tasty_memory::writer::MemoryRequest,
        finish: impl FnOnce(&AppServices, MemoryCompletion) -> JsonRpcResponse + Send + 'static,
    ) -> JsonRpcResponse {
        // 경로를 적어 라우팅 키 검사가 이름이 같은 `agent` 핸들러로 따라가지 않게 한다.
        let mut ticket =
            match tasty_memory::writer::MemoryAccess::submit(self.memory_access(), request) {
                Ok(ticket) => ticket,
                Err(error) => return submit_error_response(id, &error),
            };
        if let Some(completion) = ticket.try_take() {
            return finish(self, completion);
        }
        self.memory_replies().park(Deferred {
            id: id.clone(),
            ticket,
            finish: Box::new(finish),
            idempotency: None,
        });
        placeholder(id)
    }

    /// 읽기-판단-쓰기 본문을 writer 안에서 한 번에 실행하고 그 결과로 응답한다. `respond`는 본문의
    /// 값이나 요청이 받은 `MemoryError`(거절·저장 실패)를 받는다. 확정 여부를 모르는 결과와 본문
    /// panic은 `map_error`로 공통 응답을 만든다.
    pub(crate) fn reply_unit<T: Send + 'static>(
        &self,
        id: Value,
        bytes: usize,
        body: impl FnMut(&mut dyn tasty_memory::MemoryStorage) -> tasty_memory::Result<T>
        + Send
        + 'static,
        respond: impl FnOnce(&AppServices, Value, Result<T, &MemoryError>) -> JsonRpcResponse
        + Send
        + 'static,
    ) -> JsonRpcResponse {
        let reply_id = id.clone();
        self.reply_with_memory(
            id,
            tasty_memory::writer::MemoryRequest::unit(bytes, body),
            move |core, completion| match tasty_memory::writer::settle(completion) {
                Ok(output) => match output.into_value::<T>() {
                    Some(value) => respond(core, reply_id, Ok(value)),
                    None => JsonRpcResponse::internal_error(reply_id, "unexpected memory output"),
                },
                Err(Failure::Rejected { error, .. } | Failure::Storage(error)) => {
                    respond(core, reply_id, Err(&error))
                }
                Err(failure) => {
                    unsettled_response(reply_id, &failure, super::memory::map_error_ref)
                }
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex, mpsc};
    use std::time::{Duration, Instant};

    use serde_json::{Value, json};
    use tasty_ipc::caller::CallerContext;
    use tasty_ipc::protocol::{JsonRpcRequest, JsonRpcResponse};
    use tasty_memory::MemoryStorage;

    use super::{Handled, ReplyTo};
    use crate::app::services::AppServices;

    /// 실제 writer 스레드를 거치는 구성. 시험 구성의 기본은 writer 없이 바로 실행한다.
    /// 수집 창을 넓혀 제출 직후에는 결과가 없도록 한다.
    fn writer_core() -> AppServices {
        writer_core_with(
            Arc::new(Mutex::new(
                tasty_memory::MemoryStore::open_in_memory().expect("store"),
            )),
            usize::MAX,
        )
    }

    fn writer_core_with(memory: Arc<Mutex<dyn MemoryStorage>>, queue_items: usize) -> AppServices {
        let changes = Arc::new(tasty_memory::writer::ChangeFeed::default());
        let wake = tasty_utils::batch_writer::WakeLatch::new(Arc::new(|| {}));
        let writer = tasty_memory::writer::spawn(
            tasty_memory::writer::SharedStore(memory.clone()),
            changes.clone(),
            tasty_utils::batch_writer::Limits {
                max_delay: Duration::from_millis(50),
                queue_items: queue_items
                    .min(tasty_utils::batch_writer::Limits::default().queue_items),
                ..Default::default()
            },
            Arc::new(tasty_utils::batch_writer::SystemClock),
            wake.notifier(),
        )
        .expect("memory writer");
        crate::ipc::handler::cli_entry_tests::test_core_builder()
            .with_memory(memory)
            .with_memory_writer(writer, changes, wake)
            .build()
            .expect("test AppServices")
    }

    fn make_request(id: u64, method: &str, params: Value, key: Option<&str>) -> JsonRpcRequest {
        JsonRpcRequest {
            caller_agent_id: None,
            response_timeout_ms: None,
            idempotency_key: key.map(str::to_owned),
            jsonrpc: "2.0".into(),
            id: Some(json!(id)),
            method: method.into(),
            params,
            session_token: None,
        }
    }

    /// 요청을 실행하고 지연됐으면 연결 응답으로 받는다. 지연 여부를 함께 돌려준다.
    fn run_request(core: &mut AppServices, req: &JsonRpcRequest) -> (JsonRpcResponse, bool) {
        let (mut state, mut session) = crate::state::tests::test_state();
        let mut engine = session.borrow_mut();
        let caller = CallerContext::local();
        let checked =
            crate::ipc::handler::check_request(core, &mut state, &mut engine, req, &caller)
                .unwrap_or_else(|r| panic!("request rejected: {r:?}"));
        match crate::ipc::handler::handle_checked_request(core, &mut state, &mut engine, &checked) {
            Handled::Now(response) => (response, false),
            Handled::Later(deferred) => {
                let (tx, rx) = mpsc::sync_channel(1);
                core.defer_reply(deferred, ReplyTo::Ipc(tx));
                let deadline = Instant::now() + Duration::from_secs(30);
                loop {
                    core.poll_memory_replies(None);
                    if let Ok(response) = rx.try_recv() {
                        return (response, true);
                    }
                    assert!(Instant::now() < deadline, "deferred reply never arrived");
                    std::thread::sleep(Duration::from_millis(1));
                }
            }
        }
    }

    // writer 경로에서도 응답은 commit 뒤에 오고, CAS·version·오류 코드는 바로 실행할 때와 같다.
    #[test]
    fn memory_requests_answer_after_the_writer_commits() {
        let _home = crate::test_support::TastyHomeGuard::new();
        let mut core = writer_core();
        let cas_put = |id, cas: Option<u64>| {
            let mut params = json!({"scope": "global", "key": "k", "value": "v"});
            if let Some(cas) = cas {
                params["cas"] = json!(cas);
            }
            make_request(id, "memory.put", params, None)
        };
        let mut deferred = 0;
        let (first, later) = run_request(&mut core, &cas_put(1, None));
        deferred += usize::from(later);
        assert_eq!(first.result.expect("put")["version"], json!(1));
        let (stale, later) = run_request(&mut core, &cas_put(2, Some(7)));
        deferred += usize::from(later);
        assert_eq!(stale.error.expect("cas").code, -32005);
        let (got, later) = run_request(
            &mut core,
            &make_request(
                3,
                "memory.get",
                json!({"scope": "global", "key": "k"}),
                None,
            ),
        );
        deferred += usize::from(later);
        assert_eq!(got.result.expect("get")["version"], json!(1));
        assert_eq!(deferred, 3, "every request goes through the writer");
    }

    // 읽기-판단-쓰기 연산(blackboard)도 writer 안에서 한 번에 판정한다. CAS 거절은 기존 코드다.
    #[test]
    fn compound_memory_requests_judge_inside_the_writer() {
        let _home = crate::test_support::TastyHomeGuard::new();
        let mut core = writer_core();
        let bb = |id, method: &str, extra: Value| {
            let mut params = json!({"workspace_id": 1, "name": "board"});
            if let (Some(p), Some(e)) = (params.as_object_mut(), extra.as_object()) {
                p.extend(e.clone());
            }
            make_request(id, method, params, None)
        };
        let (created, later) = run_request(&mut core, &bb(1, "memory.bb_create", json!({})));
        assert!(later);
        assert!(created.error.is_none(), "{created:?}");
        let (first, _) = run_request(
            &mut core,
            &bb(2, "memory.bb_put", json!({"field": "f", "value": "1"})),
        );
        let version = first.result.expect("bb_put")["version"].clone();
        let (stale, _) = run_request(
            &mut core,
            &bb(
                3,
                "memory.bb_put",
                json!({"field": "f", "value": "2", "cas": 99}),
            ),
        );
        assert_eq!(stale.error.expect("cas").code, -32005);
        let (got, _) = run_request(&mut core, &bb(4, "memory.bb_get", json!({"field": "f"})));
        let got = got.result.expect("bb_get");
        assert_eq!(got["version"], version);
        assert_eq!(got["value"], json!("1"));
    }

    // 멱등 키 기록은 continuation의 응답으로 끝난다. 같은 키의 재시도는 실행하지 않고 같은 답을
    // 받고, 결과를 기다리는 동안의 재시도는 결과 불명 오류를 받는다.
    #[test]
    fn a_deferred_reply_records_its_idempotency_answer() {
        let _home = crate::test_support::TastyHomeGuard::new();
        let mut core = writer_core();
        let params = json!({
            "entries": [{"scope": "global", "key": "idem", "kind": "text", "value": "v"}],
        });
        let import = || make_request(1, "memory.import", params.clone(), Some("deferred-idem"));
        let (first, _) = run_request(&mut core, &import());
        assert_eq!(first.result.clone().expect("import")["applied"], json!(1));
        let (again, later) = run_request(&mut core, &import());
        assert!(!later, "a replay must not reach the writer");
        assert_eq!(first.result, again.result);

        let pending = import_while_pending(&mut core);
        assert_eq!(
            pending.error.expect("in flight").code,
            tasty_ipc::protocol::ERR_RESPONSE_TIMEOUT_OUTCOME_UNKNOWN
        );
    }

    fn put_request(id: u64, key: &str) -> JsonRpcRequest {
        make_request(
            id,
            "memory.put",
            json!({"scope": "global", "key": key, "value": "v"}),
            None,
        )
    }

    fn error_of(response: JsonRpcResponse) -> (i32, String, Option<Value>) {
        let e = response.error.expect("error response");
        (e.code, e.message, e.data)
    }

    // 종료 중 접수 거절: -32603, writer:closed.
    #[test]
    fn a_closed_writer_answers_with_the_closed_error() {
        let _home = crate::test_support::TastyHomeGuard::new();
        let mut core = writer_core();
        let deadline = Instant::now() + Duration::from_secs(30);
        while core.poll_memory_writer_shutdown() != 0 {
            assert!(Instant::now() < deadline, "writer did not stop");
            std::thread::sleep(Duration::from_millis(1));
        }
        let (response, later) = run_request(&mut core, &put_request(1, "k"));
        assert!(!later);
        let (code, message, data) = error_of(response);
        assert_eq!(code, -32603);
        assert_eq!(message, "memory writer is not accepting requests");
        assert_eq!(
            data,
            Some(json!({"storage_failure": "other", "writer": "closed"}))
        );
    }

    // 큐 포화: -32603, storage_failure:busy(재시도 가능), writer:saturated. 먼저 접수한 요청은 그대로 끝난다.
    #[test]
    fn a_saturated_writer_answers_busy_and_keeps_the_accepted_request() {
        let _home = crate::test_support::TastyHomeGuard::new();
        let mut core = writer_core_with(
            Arc::new(Mutex::new(
                tasty_memory::MemoryStore::open_in_memory().expect("store"),
            )),
            1,
        );
        let (mut state, mut session) = crate::state::tests::test_state();
        let mut engine = session.borrow_mut();
        let caller = CallerContext::local();
        let first_req = put_request(1, "a");
        let checked = crate::ipc::handler::check_request(
            &mut core,
            &mut state,
            &mut engine,
            &first_req,
            &caller,
        )
        .unwrap_or_else(|r| panic!("request rejected: {r:?}"));
        let Handled::Later(first) = crate::ipc::handler::handle_checked_request(
            &mut core,
            &mut state,
            &mut engine,
            &checked,
        ) else {
            panic!("the first request was not deferred");
        };
        let second_req = put_request(2, "b");
        let checked = crate::ipc::handler::check_request(
            &mut core,
            &mut state,
            &mut engine,
            &second_req,
            &caller,
        )
        .unwrap_or_else(|r| panic!("request rejected: {r:?}"));
        let Handled::Now(refused) = crate::ipc::handler::handle_checked_request(
            &mut core,
            &mut state,
            &mut engine,
            &checked,
        ) else {
            panic!("a request over the budget was accepted");
        };
        let (code, message, data) = error_of(refused);
        assert_eq!(code, -32603);
        assert!(
            message.starts_with("memory writer queue is full ("),
            "{message}"
        );
        assert_eq!(
            data,
            Some(json!({"storage_failure": "busy", "writer": "saturated"}))
        );
        let (tx, rx) = mpsc::sync_channel(1);
        core.defer_reply(first, ReplyTo::Ipc(tx));
        let deadline = Instant::now() + Duration::from_secs(30);
        let accepted = loop {
            core.poll_memory_replies(None);
            if let Ok(response) = rx.try_recv() {
                break response;
            }
            assert!(Instant::now() < deadline, "deferred reply never arrived");
            std::thread::sleep(Duration::from_millis(1));
        };
        assert_eq!(accepted.result.expect("put")["version"], json!(1));
    }

    // 요청 본문 panic: -32603 'memory request panicked'. 시험 저장소의 미구현 import가 panic한다.
    #[test]
    fn a_panicking_request_answers_with_the_panic_error() {
        let _home = crate::test_support::TastyHomeGuard::new();
        let mut core = writer_core_with(
            Arc::new(Mutex::new(tasty_memory::testing::InMemoryStorage::new())),
            usize::MAX,
        );
        let params = json!({
            "entries": [{"scope": "global", "key": "p", "kind": "text", "value": "v"}],
        });
        let (response, later) =
            run_request(&mut core, &make_request(1, "memory.import", params, None));
        assert!(later);
        let (code, message, data) = error_of(response);
        assert_eq!(code, -32603);
        assert_eq!(message, "memory request panicked");
        assert_eq!(data, Some(json!({"storage_failure": "other"})));
    }

    // 결과 불명: 원인 오류가 있으면 기존 문구에 outcome:unknown을, worker 소실이면 고정 문구를 쓴다.
    #[test]
    fn unknown_outcomes_say_the_write_may_have_landed() {
        use tasty_memory::writer::Failure;
        let busy = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY),
            None,
        );
        let expected = format!("memory db error: {busy}");
        let cause = Arc::new(tasty_memory::MemoryError::Db(busy));
        let (code, message, data) = error_of(super::unsettled_response(
            json!(1),
            &Failure::Unknown(Some(cause)),
            super::super::memory::map_error_ref,
        ));
        assert_eq!(code, -32603);
        assert_eq!(message, expected);
        assert_eq!(
            data,
            Some(json!({"storage_failure": "busy", "outcome": "unknown"}))
        );

        let (code, message, data) = error_of(super::unsettled_response(
            json!(2),
            &Failure::Unknown(None),
            super::super::memory::map_error_ref,
        ));
        assert_eq!(code, -32603);
        assert_eq!(
            message,
            "memory writer ended before the request finished; it may have been stored"
        );
        assert_eq!(
            data,
            Some(json!({"storage_failure": "other", "outcome": "unknown"}))
        );

        // 저장 실패와 요청 거절은 기존 매핑 그대로다.
        let refused = Arc::new(tasty_memory::MemoryError::NotFound {
            scope: "global".into(),
            key: "k".into(),
        });
        let (code, _, data) = error_of(super::unsettled_response(
            json!(3),
            &Failure::Rejected {
                error: refused,
                index: None,
            },
            super::super::memory::map_error_ref,
        ));
        assert_eq!((code, data), (-32004, None));
    }

    // 세 문서의 오류 표가 서로 같고, 고정 문구가 코드가 만드는 문구와 같다.
    #[test]
    fn the_writer_error_table_matches_in_every_document_and_in_code() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let docs = [
            "docs/design/systems/storage-contract.md",
            "docs/reference/api.md",
            "docs/dev-guide/headless-ipc-surface.md",
        ];
        let tables: Vec<Vec<String>> = docs
            .iter()
            .map(|path| {
                let text = std::fs::read_to_string(root.join(path))
                    .unwrap_or_else(|e| panic!("{path}: {e}"));
                let start = text
                    .find("| 큐 포화(")
                    .unwrap_or_else(|| panic!("{path} has no writer error table"));
                text[start..]
                    .lines()
                    .take_while(|l| l.starts_with('|'))
                    .map(str::to_owned)
                    .collect()
            })
            .collect();
        assert_eq!(tables[0].len(), 6, "{:#?}", tables[0]);
        for (path, table) in docs.iter().zip(&tables).skip(1) {
            assert_eq!(table, &tables[0], "{path} differs from {}", docs[0]);
        }
        let closed =
            super::submit_error_response(json!(1), &tasty_utils::batch_writer::SubmitError::Closed);
        let saturated = super::submit_error_response(
            json!(1),
            &tasty_utils::batch_writer::SubmitError::Saturated { items: 7, bytes: 9 },
        );
        let lost = super::unsettled_response(
            json!(1),
            &tasty_memory::writer::Failure::Unknown(None),
            super::super::memory::map_error_ref,
        );
        let panicked = super::unsettled_response(
            json!(1),
            &tasty_memory::writer::Failure::Panicked,
            super::super::memory::map_error_ref,
        );
        let documented = tables[0].join("\n");
        let documented_data: Vec<Value> = documented
            .split('`')
            .filter(|cell| cell.starts_with('{'))
            .map(|cell| serde_json::from_str(cell).unwrap_or_else(|e| panic!("{cell}: {e}")))
            .collect();
        assert_eq!(documented_data.len(), 4, "{documented_data:?}");
        for response in [closed, lost, panicked] {
            let (_, message, data) = error_of(response);
            assert!(documented.contains(&format!("`{message}`")), "{message}");
            let data = data.expect("data");
            assert!(documented_data.contains(&data), "{data}");
        }
        let (_, message, data) = error_of(saturated);
        assert_eq!(
            message,
            "memory writer queue is full (7 requests, 9 bytes held)"
        );
        assert!(documented_data.contains(&data.expect("data")));
    }

    /// 결과를 거두기 전에 같은 키로 다시 보낸 요청의 응답.
    fn import_while_pending(core: &mut AppServices) -> JsonRpcResponse {
        let params = json!({
            "entries": [{"scope": "global", "key": "idem2", "kind": "text", "value": "v"}],
        });
        let req = make_request(2, "memory.import", params, Some("pending-idem"));
        let (mut state, mut session) = crate::state::tests::test_state();
        let mut engine = session.borrow_mut();
        let caller = CallerContext::local();
        let checked =
            crate::ipc::handler::check_request(core, &mut state, &mut engine, &req, &caller)
                .unwrap_or_else(|r| panic!("request rejected: {r:?}"));
        let first =
            crate::ipc::handler::handle_checked_request(core, &mut state, &mut engine, &checked);
        let Handled::Later(deferred) = first else {
            panic!("writer route answered without deferring");
        };
        let checked =
            crate::ipc::handler::check_request(core, &mut state, &mut engine, &req, &caller)
                .unwrap_or_else(|r| panic!("request rejected: {r:?}"));
        let second =
            crate::ipc::handler::handle_checked_request(core, &mut state, &mut engine, &checked);
        let (tx, rx) = mpsc::sync_channel(1);
        core.defer_reply(deferred, ReplyTo::Ipc(tx));
        let deadline = Instant::now() + Duration::from_secs(30);
        while rx.try_recv().is_err() {
            core.poll_memory_replies(None);
            assert!(Instant::now() < deadline, "deferred reply never arrived");
            std::thread::sleep(Duration::from_millis(1));
        }
        match second {
            Handled::Now(response) => response,
            Handled::Later(_) => panic!("a retry in flight reached the writer"),
        }
    }
}
