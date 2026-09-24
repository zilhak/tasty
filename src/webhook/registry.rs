//! 리스너와 IPC 핸들러가 공유하는 웹훅 등록 목록.
//! Mutex로 접근을 직렬화하며 만료는 요청·복원·sweep에서 확인한다.

use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard, OnceLock};

use super::auth::WebhookAuth;
use super::lifetime::{Lifetime, now_unix};
use crate::hook_handler::{HookHandlerId, IpcCall};
use tasty_ipc::host_call::HostIpcInjector;

#[derive(Debug, Clone)]
pub struct WebhookEntry {
    /// URL 경로로 쓰는 랜덤 16자리 hex ID.
    pub id: String,
    /// 허용 HTTP 메서드(대문자 정규화).
    pub methods: Vec<String>,
    /// 레지스트리 핸들러 참조(핸들러 id 로 등록한 경우). 인라인 시퀀스면 익명 id.
    pub handler_id: Option<HookHandlerId>,
    /// 등록할 때 확정한 실행 시퀀스.
    pub calls: Vec<IpcCall>,
    pub lifetime: Lifetime,
    /// 선택적 인증 설정. `None` 이면 무인증 통과(인증은 opt-in).
    pub auth: Option<WebhookAuth>,
}

#[derive(Default)]
struct WebhookState {
    /// bind 주소(예: `0.0.0.0`).
    bind_addr: String,
    /// 설정 포트(bind 성공 여부와 무관하게 URL 표기에 사용).
    port: Option<u16>,
    /// path(opaque id) → 엔트리.
    entries: BTreeMap<String, WebhookEntry>,
    /// off-main thread → 메인루프 IPC 주입기.
    injector: Option<HostIpcInjector>,
    /// tiny_http 서버가 실제로 bind 되었는가(중복 bind 가드).
    bound: bool,
}

static STATE: OnceLock<Mutex<WebhookState>> = OnceLock::new();

fn state() -> &'static Mutex<WebhookState> {
    STATE.get_or_init(|| Mutex::new(WebhookState::default()))
}

const STATE_WHAT: &str = "the webhook registry";
static STATE_POISON_REPORTED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// poison을 보고하고 등록 목록을 복구한다. 부분 갱신 상태까지 되돌리지는 않는다.
/// 시험은 지역 Mutex를 주입해 전역 등록 상태에 영향을 주지 않는다.
fn lock_state(state: &Mutex<WebhookState>) -> MutexGuard<'_, WebhookState> {
    tasty_utils::poison::recover_mutex(state.lock(), STATE_WHAT, &STATE_POISON_REPORTED)
}

fn lock() -> MutexGuard<'static, WebhookState> {
    lock_state(state())
}

/// bind 전에 주소·포트·injector를 설정한다. port가 None이면 URL에도 포트가 빠진다.
pub(super) fn set_runtime(injector: HostIpcInjector, bind_addr: &str, port: Option<u16>) {
    let mut s = lock();
    s.injector = Some(injector);
    s.bind_addr = bind_addr.to_string();
    s.port = port;
}

pub(super) fn is_bound() -> bool {
    lock().bound
}

/// 현재 설정된 포트(`webhook.config` get 용). 미설정이면 `None`.
pub fn configured_port() -> Option<u16> {
    lock().port
}

pub fn is_listener_bound() -> bool {
    lock().bound
}

pub(super) fn mark_bound() {
    lock().bound = true;
}

/// 8바이트 난수로 16자리 hex ID를 만들고 현재 등록 목록과 충돌하면 다시 시도한다.
fn gen_opaque_id(entries: &BTreeMap<String, WebhookEntry>) -> String {
    use rand::Rng;
    let mut rng = rand::rng();
    loop {
        let bytes: [u8; 8] = rng.random();
        let id: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        if !entries.contains_key(&id) {
            return id;
        }
    }
}

/// URL 표기용 host — `0.0.0.0` bind 는 curl/클릭 가능하도록 loopback 으로 치환.
fn display_host(bind_addr: &str) -> &str {
    if bind_addr == "0.0.0.0" || bind_addr.is_empty() {
        "127.0.0.1"
    } else {
        bind_addr
    }
}

fn build_url(s: &WebhookState, id: &str) -> String {
    let host = display_host(&s.bind_addr);
    match s.port {
        Some(port) => format!("http://{host}:{port}/{id}"),
        None => format!("http://{host}/{id}"),
    }
}

#[derive(Debug, Clone)]
pub struct RegisterOutcome {
    pub id: String,
    pub url: String,
}

/// 락 안에서 Persistent 항목의 저장을 시도한다. Temporary만 남았으면 webhook 키를 지운다.
fn persist_locked(s: &WebhookState) {
    let persistent: Vec<_> = s
        .entries
        .values()
        .filter(|e| e.lifetime.is_persistent())
        .map(super::persist::to_persisted)
        .collect();
    super::persist::write(&persistent);
}

/// 재시작 필터 후 등 외부에서 현재 영속 상태를 파일에 재기록한다.
pub(super) fn persist_now() {
    let s = lock();
    persist_locked(&s);
}

/// ID를 발급해 등록하고 URL을 반환한다. Persistent면 저장도 시도한다.
pub fn register(
    methods: Vec<String>,
    handler_id: Option<HookHandlerId>,
    calls: Vec<IpcCall>,
    lifetime: Lifetime,
    auth: Option<WebhookAuth>,
) -> RegisterOutcome {
    let mut s = lock();
    let id = gen_opaque_id(&s.entries);
    let url = build_url(&s, &id);
    s.entries.insert(
        id.clone(),
        WebhookEntry {
            id: id.clone(),
            methods,
            handler_id,
            calls,
            lifetime,
            auth,
        },
    );
    if lifetime.is_persistent() {
        persist_locked(&s);
    }
    RegisterOutcome { id, url }
}

/// 재시작 복원용 — 영속화된 엔트리를 그대로 in-memory 로 복원한다(id 유지, 저장
/// 재기록 없음). 이미 등록된 id 는 건너뛴다.
pub(super) fn restore_entry(entry: WebhookEntry) {
    let mut s = lock();
    s.entries.entry(entry.id.clone()).or_insert(entry);
}

/// 모든 등록과 표시용 URL을 반환한다. 만료 여부를 여기서 검사하지는 않는다.
pub fn list() -> Vec<(WebhookEntry, String)> {
    let s = lock();
    s.entries
        .values()
        .map(|e| (e.clone(), build_url(&s, &e.id)))
        .collect()
}

/// 단일 웹훅 상세 (id 지정).
pub fn info(id: &str) -> Option<(WebhookEntry, String)> {
    let s = lock();
    s.entries.get(id).map(|e| (e.clone(), build_url(&s, &e.id)))
}

/// 항목이 있었으면 제거하고 true를 반환한다. Persistent면 파일 갱신도 시도한다.
pub fn unregister(id: &str) -> bool {
    let mut s = lock();
    match s.entries.remove(id) {
        Some(removed) => {
            if removed.lifetime.is_persistent() {
                persist_locked(&s);
            }
            true
        }
        None => false,
    }
}

/// 만료된 항목을 제거하고 ID 목록을 반환한다. Persistent를 제거하면 파일도 갱신하려고 시도한다.
pub fn sweep() -> Vec<String> {
    let now = now_unix();
    let mut s = lock();
    let expired: Vec<String> = s
        .entries
        .iter()
        .filter(|(_, e)| e.lifetime.is_expired(now))
        .map(|(id, _)| id.clone())
        .collect();
    let mut persistent_removed = false;
    for id in &expired {
        if let Some(removed) = s.entries.remove(id)
            && removed.lifetime.is_persistent()
        {
            persistent_removed = true;
        }
    }
    if persistent_removed {
        persist_locked(&s);
    }
    expired
}

/// 리스너 thread 의 요청 매칭 결과.
pub(super) enum MatchResult {
    NotFound,
    MethodNotAllowed,
    /// lifetime 만료(시간 초과 / 횟수 소진) — 410 Gone. 매칭 시 lazy 삭제됨.
    Expired,
    /// 인증 실패. 등록 횟수는 차감하지 않는다.
    Unauthorized,
    Matched {
        calls: Vec<IpcCall>,
        injector: Option<HostIpcInjector>,
    },
}

/// 만료·메서드·인증을 확인하고 실행 시퀀스와 injector를 반환한다.
/// 인증과 횟수 차감을 같은 락 안에서 처리한다. 거부된 요청은 횟수를 차감하지 않는다.
/// 통과한 요청은 실제 실행 전에 차감하며 소진되면 등록을 지워 다음 요청은 404가 된다.
/// injector가 없거나 이후 실행에 실패해도 차감을 되돌리지 않는다.
pub(super) fn match_request(
    path: &str,
    method: &str,
    authorized: impl FnOnce(&WebhookAuth) -> bool,
) -> MatchResult {
    let now = now_unix();
    let mut s = lock();
    let Some(entry) = s.entries.get(path) else {
        return MatchResult::NotFound;
    };

    if entry.lifetime.is_time_expired(now) || entry.lifetime.is_exhausted() {
        let persistent = entry.lifetime.is_persistent();
        s.entries.remove(path);
        if persistent {
            persist_locked(&s);
        }
        return MatchResult::Expired;
    }

    if !entry.methods.iter().any(|m| m == method) {
        return MatchResult::MethodNotAllowed;
    }

    if let Some(a) = &entry.auth
        && !authorized(a)
    {
        return MatchResult::Unauthorized;
    }

    let injector = s.injector.clone();
    let entry = s.entries.get_mut(path).expect("entry present under lock");
    let calls = entry.calls.clone();
    let exhausted = entry.lifetime.consume();
    let persistent = entry.lifetime.is_persistent();
    if exhausted {
        s.entries.remove(path);
    }
    if persistent {
        persist_locked(&s);
    }
    MatchResult::Matched { calls, injector }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::webhook::lifetime::{Limit, Persistence};

    // sweep이 전역 목록을 정리하므로 다른 시험의 항목에 간섭하지 않게 직렬화한다.
    static TEST_SERIAL: Mutex<()> = Mutex::new(());

    fn serial() -> MutexGuard<'static, ()> {
        TEST_SERIAL.lock().unwrap_or_else(|p| p.into_inner())
    }

    // 실제 설정 파일을 쓰지 않도록 Temporary를 사용한다.
    fn temp(limit: Limit) -> Lifetime {
        Lifetime {
            persistence: Persistence::Temporary,
            limit,
        }
    }

    // 전역 목록 대신 지역 Mutex를 poison해 시험 간 영향을 막는다.
    #[test]
    fn a_poisoned_registry_still_reads_and_writes() {
        let shared = std::sync::Arc::new(Mutex::new(WebhookState::default()));

        let poisoner = std::sync::Arc::clone(&shared);
        std::thread::spawn(move || {
            let _guard = poisoner.lock().expect("아직 성한 락");
            panic!("이 스레드가 락을 쥔 채 죽는다");
        })
        .join()
        .expect_err("패닉한 스레드는 Err 로 join 된다");
        assert!(shared.lock().is_err(), "poison 이 실제로 걸려야 한다");

        lock_state(&shared).port = Some(8123);
        assert_eq!(
            lock_state(&shared).port,
            Some(8123),
            "poison 뒤에도 설정이 읽고 쓰여야 한다"
        );

        assert!(
            STATE_POISON_REPORTED.load(std::sync::atomic::Ordering::Relaxed),
            "poison 복구 사실을 한 번은 보고해야 한다"
        );
    }

    #[test]
    fn register_list_info_unregister_roundtrip() {
        let _g = serial();
        let calls = vec![IpcCall {
            method: "notification.create".to_string(),
            params: serde_json::json!({"body": "${body.message}"}),
        }];
        let out = register(
            vec!["POST".to_string()],
            None,
            calls.clone(),
            temp(Limit::Unlimited),
            None,
        );
        assert_eq!(out.id.len(), 16); // 8바이트 → 16 hex
        assert!(out.url.contains(&out.id));

        assert!(info(&out.id).is_some());
        assert!(list().iter().any(|(e, _)| e.id == out.id));

        assert!(matches!(
            match_request(&out.id, "POST", |_| true),
            MatchResult::Matched { .. }
        ));
        assert!(matches!(
            match_request(&out.id, "GET", |_| true),
            MatchResult::MethodNotAllowed
        ));
        assert!(matches!(
            match_request("nope", "POST", |_| true),
            MatchResult::NotFound
        ));

        assert!(unregister(&out.id));
        assert!(info(&out.id).is_none());
        assert!(matches!(
            match_request(&out.id, "POST", |_| true),
            MatchResult::NotFound
        ));
    }

    #[test]
    fn opaque_ids_are_nonsequential() {
        let _g = serial();
        let a = register(
            vec!["POST".to_string()],
            None,
            vec![],
            temp(Limit::Unlimited),
            None,
        );
        let b = register(
            vec!["POST".to_string()],
            None,
            vec![],
            temp(Limit::Unlimited),
            None,
        );
        assert_ne!(a.id, b.id);
        unregister(&a.id);
        unregister(&b.id);
    }

    #[test]
    fn count_limit_consumes_and_self_destructs() {
        let _g = serial();
        let out = register(
            vec!["POST".to_string()],
            None,
            vec![],
            temp(Limit::CountLimit { remaining: 2 }),
            None,
        );
        assert!(matches!(
            match_request(&out.id, "POST", |_| true),
            MatchResult::Matched { .. }
        ));
        assert!(matches!(
            info(&out.id).unwrap().0.lifetime.limit,
            Limit::CountLimit { remaining: 1 }
        ));
        assert!(matches!(
            match_request(&out.id, "POST", |_| true),
            MatchResult::Matched { .. }
        ));
        assert!(info(&out.id).is_none());
        assert!(matches!(
            match_request(&out.id, "POST", |_| true),
            MatchResult::NotFound
        ));
    }

    #[test]
    fn time_limit_lazy_expires_with_410() {
        let _g = serial();
        let out = register(
            vec!["POST".to_string()],
            None,
            vec![],
            temp(Limit::TimeLimit { deadline_unix: 1 }),
            None,
        );
        assert!(matches!(
            match_request(&out.id, "POST", |_| true),
            MatchResult::Expired
        ));
        assert!(info(&out.id).is_none());
        assert!(matches!(
            match_request(&out.id, "POST", |_| true),
            MatchResult::NotFound
        ));
    }

    #[test]
    fn method_mismatch_does_not_consume_count() {
        let _g = serial();
        let out = register(
            vec!["POST".to_string()],
            None,
            vec![],
            temp(Limit::CountLimit { remaining: 1 }),
            None,
        );
        assert!(matches!(
            match_request(&out.id, "GET", |_| true),
            MatchResult::MethodNotAllowed
        ));
        assert!(matches!(
            info(&out.id).unwrap().0.lifetime.limit,
            Limit::CountLimit { remaining: 1 }
        ));
        unregister(&out.id);
    }

    #[test]
    fn sweep_removes_only_expired() {
        let _g = serial();
        let live = register(
            vec!["POST".to_string()],
            None,
            vec![],
            temp(Limit::Unlimited),
            None,
        );
        let expired = register(
            vec!["POST".to_string()],
            None,
            vec![],
            temp(Limit::TimeLimit { deadline_unix: 1 }),
            None,
        );
        let swept = sweep();
        assert!(swept.contains(&expired.id));
        assert!(!swept.contains(&live.id));
        assert!(info(&expired.id).is_none());
        assert!(info(&live.id).is_some());
        unregister(&live.id);
    }
}
