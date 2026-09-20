//! 같은 변경 요청의 재시도가 **중복 실행인지 재조회인지** 갈리게 하는 보존소.
//!
//! ## 이 모듈이 없을 때 무엇이 구별되지 않았나
//!
//! 응답만 유실된 요청을 호출자가 다시 보내면, 받는 쪽에는 그것이 새 요청인지 재시도인지
//! 말해 주는 것이 **하나도 없었다.** `id` 는 응답 대응용이라 연결마다 다시 매겨지고,
//! params 가 같다는 사실은 "같은 일을 두 번 하려는 것" 과 구별되지 않는다. 그래서
//! 재시도는 그대로 두 번째 효과를 남겼고, 호출자는 그것을 관측할 수단도 없었다.
//!
//! 구별을 만들 수 있는 것은 **호출자뿐**이다 — 자기가 보내는 두 요청이 같은 것인지는
//! 자기만 안다. [`tasty_ipc::protocol::JsonRpcRequest::idempotency_key`] 가 그 선언이고,
//! 이 모듈은 그 선언을 받아 **처음 본 키만 실행**한다.
//!
//! ## 무엇에 대해 도는가 — 모수는 `Mutate` 다
//!
//! 키가 뜻을 갖는 메서드는 [`MethodEffect::Mutate`] 로 분류된 것뿐이다. 나머지 둘은
//! 정의상 재전달이 안전하고(읽기는 흔적을 안 남기고, 멱등은 같은 끝 상태로 수렴한다),
//! 보존소에 넣으면 오히려 **조회가 낡은 답을 받는다.** 그 분류가 이미 표에 있으므로
//! 여기서 목록을 다시 만들지 않는다 — 두 목록이면 갈리고, 갈렸을 때 조용하다.
//!
//! ## 동시에 같은 키가 둘 오면
//!
//! 수렴한다. 다만 그것을 **이 모듈이 만드는 것이 아니다** — 호스트의 IPC 실행은
//! [`super::handle_checked_request`] 한 자리로 모이고 거기서 직렬로 돈다(요청 하나가
//! 끝나야 다음 것이 시작한다). 그래서 둘째 요청이 보존소를 볼 때 첫째는 이미 끝나 있고,
//! 관측 가능한 "진행 중" 상태가 없다. 진행 중을 뜻하는 값을 따로 두지 않은 이유가
//! 이것이다 — **도달할 수 없는 상태를 만들면 그 갈래는 영원히 안 재진다.**
//!
//! ## 보장의 경계 — 없는 것을 약속하지 않는다
//!
//! 보존소는 프로세스 메모리에 있고 상한이 셋이다(보존 시간 · 항목 수 · 총 바이트).
//! 경계 밖에서 일어나는 일은 둘이고 **답이 다르다**:
//!
//! - 항목이 밀려났으면 그 키는 **처음 보는 키와 구별되지 않는다** → 다시 실행된다.
//!   구별하려면 밀려난 키를 영원히 기억해야 하고, 그것은 상한이 있다는 말과 모순이다.
//! - 답만 버렸으면(너무 커서) 키는 남는다 → [`ERR_IDEMPOTENT_RESULT_DISCARDED`] 로
//!   **"실행은 됐고 답이 없다"** 를 말한다. 이쪽은 재전송이 답이 아니다.
//!
//! 호스트가 재시작하면 전부 사라진다. 그 경계를 호출자가 읽을 수 있게 [`declaration`]
//! 이 값으로 내놓고 `system.info` 가 싣는다.

use std::collections::VecDeque;
use std::sync::atomic::AtomicBool;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use tasty_ipc::caller::CallerContext;
use tasty_ipc::method_meta::{MethodEffect, method_meta};
use tasty_ipc::protocol::{
    ERR_IDEMPOTENCY_KEY_CONFLICT, ERR_IDEMPOTENT_RESULT_DISCARDED, JsonRpcRequest, JsonRpcResponse,
};

/// 보관한 답이 유효한 시간.
///
/// 값의 근거는 이 보존소가 막으려는 사건의 수명이다 — 응답을 놓친 호출자가 다시 거는
/// 간격(연결 재수립 + 재시도 백오프)이고, 사람이 손으로 다시 거는 경우까지 덮으려면
/// 분 단위여야 한다. 더 길게 잡을 근거는 측정된 적이 없고, 늘리면 항목 수 상한이
/// 먼저 걸려 **시간이 아니라 용량이 경계를 정하게 된다**(그쪽은 호출자가 예측할 수
/// 없다).
pub(crate) const RETENTION: Duration = Duration::from_secs(600);

/// 동시에 기억하는 키의 수.
pub(crate) const CAPACITY: usize = 256;

/// 답 하나를 보관할 최대 바이트. 넘으면 키만 남기고 답을 버린다.
///
/// `Mutate` 중에는 터미널 출력을 통째로 내는 것이 있다(`surface.read_since_scan_mark`).
/// 그런 답까지 항목마다 들고 있으면 상한이 항목 수로는 안 선다.
pub(crate) const MAX_STORED_RESPONSE_BYTES: usize = 64 * 1024;

/// 보관한 답 전체의 합 상한. 항목 수 상한과 **둘 다** 건다 — 한쪽만으로는 작은 답
/// 256 개와 큰 답 256 개가 같은 상한 아래 놓인다.
pub(crate) const TOTAL_BUDGET_BYTES: usize = 4 * 1024 * 1024;

/// 키 문자열의 최대 바이트. 키는 호출자가 고르므로 상한이 없으면 보존소의 크기를
/// 호출자가 정하게 된다.
pub(crate) const MAX_KEY_BYTES: usize = 256;

/// 보관 중인 한 요청의 결말.
#[derive(Debug)]
enum Stored {
    /// 답을 그대로 들고 있다.
    Response(Box<JsonRpcResponse>),
    /// 실행은 됐지만 답이 상한을 넘어 버렸다.
    Discarded,
}

#[derive(Debug)]
struct Entry {
    /// 키를 고른 주체. [`caller_scope`] 참조.
    scope: String,
    key: String,
    /// `(메서드, params)` 의 다이제스트. 같은 키에 다른 요청이 붙는 것을 잡는다.
    ///
    /// 원문을 안 들고 다이제스트로 견주는 이유는 params 가 요청 줄 상한까지 커질 수
    /// 있어서다. 다이제스트가 충돌하면 다른 요청이 앞선 답을 받지만, 그러려면 호출자가
    /// **이미 키를 잘못 재사용한 상태**여야 하고(맞게 쓰면 params 도 같다) 그 위에
    /// 64 비트 충돌이 겹쳐야 한다. 그 조합에 원문 보관 비용을 쓰지 않는다.
    digest: u64,
    stored_at: Instant,
    outcome: Stored,
    /// 이 항목이 [`TOTAL_BUDGET_BYTES`] 에서 차지하는 몫.
    bytes: usize,
}

/// 키 하나에 대한 판정.
#[derive(Debug)]
pub(crate) enum Decision {
    /// 처음 보는 키다. 실행하고 이 다이제스트로 기록한다.
    Execute(u64),
    /// 같은 키·같은 요청의 답이 있다. **실행하지 않는다.**
    Replay(Box<JsonRpcResponse>),
    /// 같은 키인데 요청이 다르다.
    Conflict,
    /// 실행은 됐고 답은 버려졌다.
    Discarded,
}

/// 키를 받는 보존소. 프로세스에 하나다.
#[derive(Debug, Default)]
pub(crate) struct Store {
    /// 삽입 순서. 밀어내는 쪽은 항상 앞이다.
    entries: VecDeque<Entry>,
    total_bytes: usize,
}

impl Store {
    pub(crate) const fn new() -> Self {
        Self {
            entries: VecDeque::new(),
            total_bytes: 0,
        }
    }

    /// 이 키로 무엇을 할지 정한다. 만료된 항목은 이 자리에서 걷힌다.
    pub(crate) fn decide(
        &mut self,
        now: Instant,
        scope: &str,
        key: &str,
        method: &str,
        params: &serde_json::Value,
    ) -> Decision {
        self.purge_expired(now);
        let digest = digest(method, params);
        match self
            .entries
            .iter()
            .find(|e| e.scope == scope && e.key == key)
        {
            None => Decision::Execute(digest),
            Some(e) if e.digest != digest => Decision::Conflict,
            Some(e) => match &e.outcome {
                Stored::Response(r) => Decision::Replay(r.clone()),
                Stored::Discarded => Decision::Discarded,
            },
        }
    }

    /// 실행이 끝난 답을 그 키로 기록한다.
    ///
    /// 게이트(권한·cap·rate)가 낸 거절은 여기 안 온다 — 그것들은 실행 **전에** 끝나므로
    /// 보관할 결말이 없고, 보관하면 권한이 생긴 뒤의 재시도까지 옛 거절을 받는다.
    pub(crate) fn record(
        &mut self,
        now: Instant,
        scope: &str,
        key: &str,
        digest: u64,
        response: &JsonRpcResponse,
    ) {
        // 같은 키의 옛 항목은 남기지 않는다 — 앞의 것이 남아 있으면 `decide` 가
        // 둘 중 어느 것을 볼지가 삽입 순서에 달린다.
        self.remove(scope, key);
        let size = serde_json::to_string(response)
            .map(|s| s.len())
            .unwrap_or(0);
        let (outcome, bytes) = if size > MAX_STORED_RESPONSE_BYTES {
            (Stored::Discarded, 0)
        } else {
            (Stored::Response(Box::new(response.clone())), size)
        };
        self.entries.push_back(Entry {
            scope: scope.to_string(),
            key: key.to_string(),
            digest,
            stored_at: now,
            outcome,
            bytes,
        });
        self.total_bytes += bytes;
        self.enforce_bounds();
    }

    fn remove(&mut self, scope: &str, key: &str) {
        if let Some(i) = self
            .entries
            .iter()
            .position(|e| e.scope == scope && e.key == key)
        {
            if let Some(e) = self.entries.remove(i) {
                self.total_bytes -= e.bytes;
            }
        }
    }

    fn purge_expired(&mut self, now: Instant) {
        // 앞에서부터 삽입 순이므로 만료도 앞에서부터다.
        while let Some(front) = self.entries.front() {
            if now.duration_since(front.stored_at) < RETENTION {
                break;
            }
            self.pop_front();
        }
    }

    fn enforce_bounds(&mut self) {
        while self.entries.len() > CAPACITY || self.total_bytes > TOTAL_BUDGET_BYTES {
            if self.pop_front().is_none() {
                break;
            }
        }
    }

    fn pop_front(&mut self) -> Option<Entry> {
        let e = self.entries.pop_front()?;
        self.total_bytes -= e.bytes;
        Some(e)
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.entries.len()
    }
}

/// `(메서드, params)` 의 프로세스 안 다이제스트.
///
/// 값은 프로세스 밖으로 안 나가므로 해시 함수의 안정성이 계약이 아니다 — 같은 실행
/// 안에서 같은 입력이 같은 값을 내면 된다.
fn digest(method: &str, params: &serde_json::Value) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    method.hash(&mut h);
    // `serde_json::Value` 는 항상 직렬화된다(NaN 을 담을 수 없다). 그래도 실패를
    // 무시하지 않는 이유는 그때 모든 params 가 같은 값으로 접혀 **서로 다른 요청이
    // 같은 키로 수렴**하기 때문이다 — 길이를 함께 섞어 그 접힘을 막는다.
    match serde_json::to_string(params) {
        Ok(s) => {
            s.hash(&mut h);
        }
        Err(_) => {
            // 직렬화가 안 되는 값은 원문으로 못 견주므로 구조를 그대로 센다.
            format!("{params:?}").hash(&mut h);
        }
    }
    h.finish()
}

/// 키를 고른 **주체**. 보존소의 자리는 `(주체, 키)` 로 정해진다.
///
/// 키는 호출자가 고르는 임의의 문자열이라, 주체를 빼면 서로 모르는 두 호출자가 같은
/// 문자열을 골랐을 때 한쪽이 **다른 쪽의 답**을 받는다. 그 자리는 조용하다 — 답이
/// 성공이고 모양도 맞다.
///
/// 연결 단위로 가르지 않는 이유는 이 계약의 목적 자체다. 재시도는 응답을 놓친 뒤에
/// 오므로 **다른 연결**로 온다 — 연결로 가르면 키가 한 번도 안 맞는다.
///
/// `Local` 이 하나로 묶이는 것은 그것이 한 주체이기 때문이다(사용자). CLI 를 두 번
/// 부른 것과 네트워크로 붙은 것이 같은 주체라는 판정은 이 레포가 이미 하고 있다 —
/// `Local` 은 권한 검사를 통째로 건너뛴다.
fn caller_scope(caller: &CallerContext) -> String {
    match caller {
        CallerContext::Local => "local".to_string(),
        CallerContext::Plugin { plugin_id, .. } => format!("plugin:{plugin_id}"),
        CallerContext::Agent { agent_id, .. } => format!("agent:{agent_id}"),
    }
}

static STORE: Mutex<Store> = Mutex::new(Store::new());
static POISON_REPORTED: AtomicBool = AtomicBool::new(false);
const WHAT: &str = "the IPC idempotency store";

fn store() -> MutexGuard<'static, Store> {
    tasty_utils::poison::recover_mutex(STORE.lock(), WHAT, &POISON_REPORTED)
}

/// 실행을 기다리는 키. [`begin`] 이 주고 [`finish`] 가 받는다.
#[derive(Debug)]
pub(crate) struct Pending {
    scope: String,
    key: String,
    digest: u64,
}

/// 라우팅 **전에** 이 요청을 실행해도 되는지 정한다.
///
/// `Ok(None)` 은 "키가 없거나 이 메서드에 뜻이 없다" 이고, 그때 동작은 이 모듈이
/// 생기기 전과 한 글자도 다르지 않다.
pub(crate) fn begin(
    now: Instant,
    caller: &CallerContext,
    request: &JsonRpcRequest,
    id: &serde_json::Value,
) -> Result<Option<Pending>, JsonRpcResponse> {
    let Some(key) = request.idempotency_key.as_deref() else {
        return Ok(None);
    };
    if key.is_empty() || key.len() > MAX_KEY_BYTES {
        return Err(JsonRpcResponse::invalid_params(
            id.clone(),
            format!(
                "idempotency_key must be 1..={MAX_KEY_BYTES} bytes (got {})",
                key.len()
            ),
        ));
    }
    // 표가 이 이름을 모르면 라우팅도 못 한다 — 그 답은 라우터가 낸다.
    if method_meta(&request.method).map(|m| m.effect) != Some(MethodEffect::Mutate) {
        return Ok(None);
    }
    let scope = caller_scope(caller);
    match store().decide(now, &scope, key, &request.method, &request.params) {
        Decision::Execute(digest) => Ok(Some(Pending {
            scope,
            key: key.to_string(),
            digest,
        })),
        Decision::Replay(stored) => Err(stored.replayed_for(id.clone())),
        Decision::Conflict => Err(JsonRpcResponse::error(
            id.clone(),
            ERR_IDEMPOTENCY_KEY_CONFLICT,
            format!(
                "idempotency_key '{key}' was already used for a different request; \
                 use a new key for a new request"
            ),
        )),
        Decision::Discarded => Err(JsonRpcResponse::error(
            id.clone(),
            ERR_IDEMPOTENT_RESULT_DISCARDED,
            format!(
                "the request for idempotency_key '{key}' did run, but its response exceeded \
                 {MAX_STORED_RESPONSE_BYTES} bytes and was not kept: do not resend, query instead"
            ),
        )),
    }
}

/// 실행이 끝났다. [`begin`] 이 키를 줬을 때만 기록한다.
pub(crate) fn finish(now: Instant, pending: Option<Pending>, response: &JsonRpcResponse) {
    if let Some(p) = pending {
        store().record(now, &p.scope, &p.key, p.digest, response);
    }
}

/// `system.info` 가 싣는 보장 범위. 상한을 **리터럴로 다시 적지 않는다** — 위 상수를
/// 그대로 내보내야 선언과 동작이 갈리지 않는다.
pub(crate) fn declaration() -> serde_json::Value {
    serde_json::json!({
        "retention_ms": RETENTION.as_millis() as u64,
        "capacity": CAPACITY,
        "max_response_bytes": MAX_STORED_RESPONSE_BYTES,
        "max_key_bytes": MAX_KEY_BYTES,
        // 보존소가 프로세스 메모리에 있다는 사실. 호출자가 crash 뒤의 보장을
        // 추정하지 않게 값으로 말한다.
        "survives_restart": false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn resp(id: i32, body: &str) -> JsonRpcResponse {
        JsonRpcResponse::success(json!(id), json!({ "body": body }))
    }

    /// 같은 키·같은 요청은 **한 번만 실행**되고 둘째는 보관된 답을 받는다.
    #[test]
    fn the_same_key_and_request_is_executed_once_and_then_replayed() {
        let mut s = Store::new();
        let t0 = Instant::now();
        let Decision::Execute(d) =
            s.decide(t0, "local", "k", "workspace.create", &json!({"name": "a"}))
        else {
            panic!("처음 보는 키는 실행이어야 한다");
        };
        s.record(t0, "local", "k", d, &resp(1, "first"));

        match s.decide(t0, "local", "k", "workspace.create", &json!({"name": "a"})) {
            Decision::Replay(r) => assert_eq!(r.result.as_ref().unwrap()["body"], "first"),
            other => panic!("재조회여야 한다: {other:?}"),
        }
    }

    /// 재조회로 나가는 답은 **이번 요청의 `id`** 를 달고, 재조회임을 표지로 말한다.
    #[test]
    fn a_replay_carries_the_new_id_and_says_it_is_a_replay() {
        let stored = resp(1, "first");
        let again = stored.replayed_for(json!(99));
        assert_eq!(
            again.id,
            json!(99),
            "앞선 요청의 id 로 답하면 대응이 깨진다"
        );
        assert!(again.idempotent_replay);
        assert!(
            !stored.idempotent_replay,
            "처음 실행된 답에 재조회 표지가 붙으면 두 사건이 구별되지 않는다"
        );
    }

    /// 같은 키에 다른 요청이 붙으면 **아무것도 실행하지 않는다.**
    #[test]
    fn the_same_key_with_a_different_request_is_a_conflict() {
        let mut s = Store::new();
        let t0 = Instant::now();
        let Decision::Execute(d) =
            s.decide(t0, "local", "k", "workspace.create", &json!({"name": "a"}))
        else {
            panic!("실행이어야 한다");
        };
        s.record(t0, "local", "k", d, &resp(1, "first"));

        assert!(matches!(
            s.decide(t0, "local", "k", "workspace.create", &json!({"name": "b"})),
            Decision::Conflict
        ));
        assert!(
            matches!(
                s.decide(t0, "local", "k", "tab.create", &json!({"name": "a"})),
                Decision::Conflict
            ),
            "메서드가 다른 것도 다른 요청이다"
        );
    }

    /// 키는 **주체마다 따로 산다.** 서로 모르는 두 호출자가 같은 문자열을 골랐을 때
    /// 한쪽이 다른 쪽의 답을 받으면, 그 자리는 성공 응답이라 아무 신호도 안 난다.
    #[test]
    fn two_callers_that_picked_the_same_string_do_not_share_an_answer() {
        let mut s = Store::new();
        let t0 = Instant::now();
        let params = json!({ "name": "same" });
        let Decision::Execute(d) = s.decide(t0, "agent:a", "k", "workspace.create", &params) else {
            panic!("실행이어야 한다");
        };
        s.record(t0, "agent:a", "k", d, &resp(1, "answer-for-a"));

        assert!(
            matches!(
                s.decide(t0, "agent:b", "k", "workspace.create", &params),
                Decision::Execute(_)
            ),
            "다른 주체가 같은 키로 남의 답을 받았다"
        );
        // 같은 주체는 그대로 재조회다 — 위 단정이 scope 를 통째로 무력화한 것이 아니다.
        assert!(matches!(
            s.decide(t0, "agent:a", "k", "workspace.create", &params),
            Decision::Replay(_)
        ));
    }

    /// 주체 문자열이 세 변종을 **갈라** 낸다. 하나로 접히면 위 시험이 통과하면서도
    /// plugin 과 agent 가 한 칸을 나눠 쓰게 된다.
    #[test]
    fn the_three_caller_kinds_get_three_scopes() {
        use std::collections::HashSet;
        use std::sync::Arc;
        let scopes: HashSet<String> = [
            caller_scope(&CallerContext::Local),
            caller_scope(&CallerContext::Plugin {
                plugin_id: "p".into(),
                permissions: Arc::new(Default::default()),
            }),
            caller_scope(&CallerContext::Agent {
                agent_id: "p".into(),
                permissions: Arc::new(Default::default()),
            }),
        ]
        .into_iter()
        .collect();
        assert_eq!(scopes.len(), 3, "주체가 접혔다: {scopes:?}");
    }

    /// 다이제스트는 **JSON 오브젝트의 키 순서에 안 흔들린다.** 흔들리면 같은 요청의
    /// 재시도가 충돌로 거절된다 — 이 축의 실패는 조용하지 않고 호출자를 막는다.
    #[test]
    fn the_digest_does_not_move_with_object_key_order() {
        let a: serde_json::Value = serde_json::from_str(r#"{"a":1,"b":2}"#).unwrap();
        let b: serde_json::Value = serde_json::from_str(r#"{"b":2,"a":1}"#).unwrap();
        assert_eq!(digest("m", &a), digest("m", &b));
        assert_ne!(digest("m", &a), digest("m", &json!({"a": 1, "b": 3})));
    }

    /// 보존 시간이 지나면 항목이 걷히고, 그 키는 **처음 보는 키와 같아진다.**
    #[test]
    fn an_expired_key_is_indistinguishable_from_a_fresh_one() {
        let mut s = Store::new();
        let t0 = Instant::now();
        let Decision::Execute(d) = s.decide(t0, "local", "k", "workspace.create", &json!({}))
        else {
            panic!("실행이어야 한다");
        };
        s.record(t0, "local", "k", d, &resp(1, "first"));
        assert!(matches!(
            s.decide(
                t0 + RETENTION - Duration::from_millis(1),
                "local",
                "k",
                "workspace.create",
                &json!({})
            ),
            Decision::Replay(_)
        ));
        assert!(matches!(
            s.decide(t0 + RETENTION, "local", "k", "workspace.create", &json!({})),
            Decision::Execute(_)
        ));
    }

    /// 항목 수 상한을 넘기면 가장 오래된 것부터 밀려난다.
    #[test]
    fn the_oldest_key_is_pushed_out_past_the_capacity() {
        let mut s = Store::new();
        let t0 = Instant::now();
        for i in 0..=CAPACITY {
            let key = format!("k{i}");
            let Decision::Execute(d) =
                s.decide(t0, "local", &key, "workspace.create", &json!({ "i": i }))
            else {
                panic!("실행이어야 한다");
            };
            s.record(t0, "local", &key, d, &resp(1, "x"));
        }
        assert_eq!(s.len(), CAPACITY);
        assert!(
            matches!(
                s.decide(t0, "local", "k0", "workspace.create", &json!({"i": 0})),
                Decision::Execute(_)
            ),
            "밀려난 키는 처음 보는 키와 구별되지 않는다"
        );
        assert!(matches!(
            s.decide(
                t0,
                "local",
                &format!("k{CAPACITY}"),
                "workspace.create",
                &json!({"i": CAPACITY})
            ),
            Decision::Replay(_)
        ));
    }

    /// 상한을 넘는 답은 **버리되 키는 남긴다** — 그래야 재전송이 두 번째 효과를
    /// 남기지 않고, 대신 "실행은 됐다" 를 말할 수 있다.
    #[test]
    fn an_oversized_response_keeps_the_key_and_drops_the_answer() {
        let mut s = Store::new();
        let t0 = Instant::now();
        let Decision::Execute(d) =
            s.decide(t0, "local", "k", "surface.read_since_scan_mark", &json!({}))
        else {
            panic!("실행이어야 한다");
        };
        let big = JsonRpcResponse::success(json!(1), json!("x".repeat(MAX_STORED_RESPONSE_BYTES)));
        s.record(t0, "local", "k", d, &big);
        assert!(matches!(
            s.decide(t0, "local", "k", "surface.read_since_scan_mark", &json!({})),
            Decision::Discarded
        ));
    }

    /// 총 바이트 상한이 항목 수 상한과 **독립으로** 건다.
    #[test]
    fn the_byte_budget_evicts_before_the_capacity_does() {
        let mut s = Store::new();
        let t0 = Instant::now();
        let payload = "y".repeat(MAX_STORED_RESPONSE_BYTES - 128);
        let n = TOTAL_BUDGET_BYTES / MAX_STORED_RESPONSE_BYTES + 2;
        assert!(
            n < CAPACITY,
            "이 시험은 항목 수 상한 아래에서 돌아야 뜻이 있다"
        );
        for i in 0..n {
            let key = format!("k{i}");
            let Decision::Execute(d) =
                s.decide(t0, "local", &key, "workspace.create", &json!({ "i": i }))
            else {
                panic!("실행이어야 한다");
            };
            s.record(
                t0,
                "local",
                &key,
                d,
                &JsonRpcResponse::success(json!(1), json!(payload)),
            );
        }
        assert!(s.len() < n, "바이트 예산이 아무것도 안 밀어냈다");
        assert!(s.total_bytes <= TOTAL_BUDGET_BYTES);
    }

    /// 키가 없으면 이 모듈은 아무 일도 안 한다.
    #[test]
    fn a_request_without_a_key_is_untouched() {
        let req = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            method: "workspace.create".into(),
            params: json!({}),
            id: Some(json!(1)),
            session_token: None,
            response_timeout_ms: None,
            idempotency_key: None,
        };
        assert!(
            begin(Instant::now(), &CallerContext::Local, &req, &json!(1))
                .unwrap()
                .is_none()
        );
    }

    /// 읽기·멱등 메서드는 키를 실어도 보존소에 안 들어간다 — 들어가면 조회가 낡은
    /// 답을 받는다.
    #[test]
    fn a_key_on_a_non_mutating_method_does_nothing() {
        for method in ["workspace.list", "workspace.update"] {
            let req = JsonRpcRequest {
                jsonrpc: "2.0".into(),
                method: method.into(),
                params: json!({}),
                id: Some(json!(1)),
                session_token: None,
                response_timeout_ms: None,
                idempotency_key: Some("shared-key".into()),
            };
            assert!(
                begin(Instant::now(), &CallerContext::Local, &req, &json!(1))
                    .unwrap()
                    .is_none(),
                "{method} 가 보존소에 들어갔다"
            );
        }
    }

    /// 빈 키와 너무 긴 키는 **실행 전에** 거절된다.
    #[test]
    fn a_key_outside_the_length_bound_is_rejected_before_anything_runs() {
        for key in ["".to_string(), "k".repeat(MAX_KEY_BYTES + 1)] {
            let req = JsonRpcRequest {
                jsonrpc: "2.0".into(),
                method: "workspace.create".into(),
                params: json!({}),
                id: Some(json!(1)),
                session_token: None,
                response_timeout_ms: None,
                idempotency_key: Some(key.clone()),
            };
            let err = begin(Instant::now(), &CallerContext::Local, &req, &json!(1))
                .expect_err("길이 밖 키는 거절이어야 한다");
            assert_eq!(err.error.expect("에러").code, -32602);
        }
    }

    /// 라우터가 실제로 보존소를 **지나는가.** 위 시험들은 보존소만 재고, 그 자리가
    /// 배선돼 있다는 것은 재지 않는다 — 진짜 handler 를 두 번 불러 **부수효과의 수**로
    /// 잰다.
    ///
    /// 통제군이 같은 시험 안에 있다: 키 없는 두 번은 워크스페이스를 둘 만든다. 그래야
    /// "하나만 생겼다" 가 보존소 덕인지 handler 가 원래 한 번만 만드는 것인지 갈린다.
    #[test]
    fn the_router_consults_the_store_before_the_handler_runs() {
        use crate::ipc::caller::CallerContext;
        use crate::ipc::handler::handle_with_caller;

        fn create(name: &str, key: Option<&str>) -> JsonRpcRequest {
            JsonRpcRequest {
                jsonrpc: "2.0".into(),
                method: "workspace.create".into(),
                params: json!({ "name": name }),
                id: Some(json!(1)),
                session_token: None,
                response_timeout_ms: None,
                idempotency_key: key.map(str::to_string),
            }
        }

        let _home = crate::test_support::TastyHomeGuard::new();
        let mut core = crate::ipc::handler::cli_entry_tests::test_core();
        let (mut state, mut engine) = crate::state::tests::test_state();
        let base = engine.workspaces.len();

        // 통제군 — 키가 없으면 두 번 불린 만큼 두 개가 생긴다.
        for _ in 0..2 {
            let r = handle_with_caller(
                &mut core,
                &mut state,
                &mut engine,
                &create("unkeyed", None),
                &CallerContext::Local,
            );
            assert!(r.error.is_none(), "준비: {:?}", r.error);
        }
        assert_eq!(
            engine.workspaces.len(),
            base + 2,
            "통제군이 성립하지 않는다"
        );

        // 실험군 — 같은 키로 두 번.
        let key = "wiring-probe-a";
        let first = handle_with_caller(
            &mut core,
            &mut state,
            &mut engine,
            &create("keyed", Some(key)),
            &CallerContext::Local,
        );
        assert!(first.error.is_none(), "{:?}", first.error);
        assert!(!first.idempotent_replay);
        let after_first = engine.workspaces.len();

        let second = handle_with_caller(
            &mut core,
            &mut state,
            &mut engine,
            &create("keyed", Some(key)),
            &CallerContext::Local,
        );
        assert_eq!(
            engine.workspaces.len(),
            after_first,
            "같은 키의 재시도가 두 번째 워크스페이스를 만들었다"
        );
        assert!(second.idempotent_replay, "재조회 표지가 없다");
        assert_eq!(second.result, first.result, "재조회가 다른 답을 냈다");
        assert_eq!(second.id, json!(1));

        // 같은 키에 다른 입력 — 아무것도 안 만들고 충돌로 답한다.
        let conflict = handle_with_caller(
            &mut core,
            &mut state,
            &mut engine,
            &create("something-else", Some(key)),
            &CallerContext::Local,
        );
        assert_eq!(
            conflict.error.expect("충돌이어야 한다").code,
            ERR_IDEMPOTENCY_KEY_CONFLICT
        );
        assert_eq!(
            engine.workspaces.len(),
            after_first,
            "충돌인데 워크스페이스가 생겼다"
        );
    }

    /// 선언은 상수에서 **유도**된다. 리터럴로 다시 적으면 동작과 갈린다.
    #[test]
    fn the_declaration_is_derived_from_the_bounds_it_describes() {
        let d = declaration();
        assert_eq!(d["retention_ms"], RETENTION.as_millis() as u64);
        assert_eq!(d["capacity"], CAPACITY);
        assert_eq!(d["max_response_bytes"], MAX_STORED_RESPONSE_BYTES);
        assert_eq!(d["max_key_bytes"], MAX_KEY_BYTES);
        assert_eq!(d["survives_restart"], false);
    }
}
