//! 웹훅 등록 상태 — 프로세스 전역 싱글턴.
//!
//! 리스너 thread(요청 매칭)와 IPC 핸들러 thread(register/list/info/unregister)가
//! 같은 상태를 공유하므로 `OnceLock<Mutex<..>>` 로 둔다. lifetime 6종
//! ([`super::lifetime`])·영속화([`super::persist`])는 S5 에서 정식화됐다 — 만료는
//! 타이머 없이 lazy(호출 시)·재시작 필터·명시 sweep 세 시점에만 확정된다.

use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard, OnceLock};

use super::auth::WebhookAuth;
use super::lifetime::{Lifetime, now_unix};
use crate::adapters::ipc::host_call::HostIpcInjector;
use crate::hook_handler::{HookHandlerId, IpcCall};

/// 등록된 웹훅 엔트리.
#[derive(Debug, Clone)]
pub struct WebhookEntry {
    /// opaque 짧은해시 path (비순차).
    pub id: String,
    /// 허용 HTTP 메서드(대문자 정규화).
    pub methods: Vec<String>,
    /// 레지스트리 핸들러 참조(핸들러 id 로 등록한 경우). 인라인 시퀀스면 익명 id.
    pub handler_id: Option<HookHandlerId>,
    /// 실행할 IpcSequence 스냅샷 (등록 시점 확정 — owner 가 고정).
    pub calls: Vec<IpcCall>,
    /// lifetime — 영속성 + 자동 소멸 제한(6종).
    pub lifetime: Lifetime,
    /// 선택적 인증 설정. `None` 이면 무인증 통과(인증은 opt-in).
    pub auth: Option<WebhookAuth>,
}

/// 웹훅 리스너 전역 상태.
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

/// 웹훅 레지스트리 락. 이 모듈의 모든 접근자가 이 한 곳을 지난다.
///
/// poison 이면 복구한다. ① 임계구역은 `entries` 조작과 그 직렬화(`persist_locked`)뿐이라
/// 최악의 손상이 "반쯤 반영된 항목 하나" 로 갇힌다. ② 패닉하면 IPC 핸들러(메인 스레드)와
/// 리스너 스레드가 죽는다 — 메인 스레드 패닉은 정책상 금지고, 리스너가 죽으면 발급된
/// 모든 URL 이 조용히 죽은 주소가 된다.
///
/// 락을 인자로 받는 이유는 전역이 [`OnceLock`] 이라서다 — 테스트가 전역을 poison 하면
/// 같은 바이너리의 뒤 테스트가 그 상태를 물려받는다. 회귀 테스트는 지역 뮤텍스를 겨냥한다.
fn lock_state(state: &Mutex<WebhookState>) -> MutexGuard<'_, WebhookState> {
    tasty_utils::poison::recover_mutex(state.lock(), STATE_WHAT, &STATE_POISON_REPORTED)
}

fn lock() -> MutexGuard<'static, WebhookState> {
    lock_state(state())
}

/// 리스너 runtime 설정 주입(부팅 헬퍼가 bind 전에 호출).
///
/// `port` 는 **설정값**(자동 폴백 없음). `None` = 포트 미설정 → bind 하지 않으며
/// 발급 URL 에도 포트가 빠진다([`build_url`]).
pub(super) fn set_runtime(injector: HostIpcInjector, bind_addr: &str, port: Option<u16>) {
    let mut s = lock();
    s.injector = Some(injector);
    s.bind_addr = bind_addr.to_string();
    s.port = port;
}

/// 이미 bind 되었는지(부팅 헬퍼 중복 호출 가드).
pub(super) fn is_bound() -> bool {
    lock().bound
}

/// 현재 설정된 포트(`webhook.config` get 용). 미설정이면 `None`.
pub fn configured_port() -> Option<u16> {
    lock().port
}

/// 리스너가 실제로 bind 되었는지(`webhook.config` get 용).
pub fn is_listener_bound() -> bool {
    lock().bound
}

/// bind 성공 표시.
pub(super) fn mark_bound() {
    lock().bound = true;
}

/// opaque 짧은해시 id 발급 — 비순차(랜덤 8바이트 → 16 hex). 충돌 시 재시도.
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

/// 발급 URL 을 구성한다.
fn build_url(s: &WebhookState, id: &str) -> String {
    let host = display_host(&s.bind_addr);
    match s.port {
        Some(port) => format!("http://{host}:{port}/{id}"),
        None => format!("http://{host}/{id}"),
    }
}

/// 등록 결과.
#[derive(Debug, Clone)]
pub struct RegisterOutcome {
    pub id: String,
    pub url: String,
}

/// lock 을 잡은 채 현재 **영속 엔트리들**을 config 에 기록한다. 상태를 mutate 하는
/// 경로(register/unregister/sweep/match count 차감)에서 영속 엔트리가 바뀌었을 때만
/// 호출한다 — `Temporary` 만 있으면 파일에서 빈 배열로 정리된다.
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

/// 웹훅 등록. opaque id 를 발급하고 (id, 메서드, 핸들러, 시퀀스, lifetime) 을
/// 저장한다. `Persistent` lifetime 이면 config 로도 영속화한다. 발급 URL 을 반환.
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

/// 전체 웹훅 목록 (포커스 독립 — 전 범위). 각 엔트리에 발급 URL 포함해 반환.
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

/// 웹훅 해제. 존재했으면 `true`. 영속 엔트리를 지웠으면 config 도 갱신한다.
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

/// 만료된(시간 초과 / 횟수 소진) 웹훅을 일괄 정리한다(`webhook.sweep`). 제거된
/// id 목록을 반환. 영속 엔트리가 하나라도 지워졌으면 config 를 갱신한다.
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
    Matched {
        calls: Vec<IpcCall>,
        injector: Option<HostIpcInjector>,
        /// 선택적 인증 설정 스냅샷. `Some` 이면 리스너가 실행 전 검증한다.
        auth: Option<WebhookAuth>,
    },
}

/// (path, method) 로 매칭해 실행할 시퀀스 스냅샷 + injector + 인증설정을 반환한다.
///
/// **lazy 만료**: path 가 불릴 때 시간제한 만료를 먼저 확인해 만료면 삭제 후
/// `Expired`(410) 를 돌린다. 매칭 성공한 횟수제한 웹훅은 카운트를 1 차감하고,
/// 소진되면 그 자리에서 삭제한다(다음 호출은 404). 짧게 lock 을 잡아 mutate +
/// clone 만 하고 실행·인증검증은 lock 밖에서 한다.
pub(super) fn match_request(path: &str, method: &str) -> MatchResult {
    let now = now_unix();
    let mut s = lock();
    let Some(entry) = s.entries.get(path) else {
        return MatchResult::NotFound;
    };

    // ① lazy 시간 만료 — 메서드 무관하게 먼저 확정(호출 시 삭제 + 410).
    if entry.lifetime.is_time_expired(now) || entry.lifetime.is_exhausted() {
        let persistent = entry.lifetime.is_persistent();
        s.entries.remove(path);
        if persistent {
            persist_locked(&s);
        }
        return MatchResult::Expired;
    }

    // ② 메서드 매칭.
    if !entry.methods.iter().any(|m| m == method) {
        return MatchResult::MethodNotAllowed;
    }

    // ③ 매칭 성공 — 횟수 차감 후 소진되면 삭제. 인증검증은 lock 밖(리스너)에서.
    let injector = s.injector.clone();
    let entry = s.entries.get_mut(path).expect("entry present under lock");
    let calls = entry.calls.clone();
    let auth = entry.auth.clone();
    let exhausted = entry.lifetime.consume();
    let persistent = entry.lifetime.is_persistent();
    if exhausted {
        s.entries.remove(path);
    }
    if persistent {
        persist_locked(&s);
    }
    MatchResult::Matched {
        calls,
        injector,
        auth,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::webhook::lifetime::{Limit, Persistence};

    /// 이 테스트들은 프로세스 전역 싱글턴(STATE)을 공유하고 `sweep()` 은 전역
    /// 만료 엔트리를 모두 제거하므로, 병렬 실행 시 서로의 엔트리에 간섭할 수 있다
    /// (예: 한 테스트의 sweep 이 다른 테스트의 만료 엔트리를 먼저 제거 → 기대 결과
    /// 어긋남). 테스트-로컬 mutex 로 직렬화해 flaky 를 방지한다.
    static TEST_SERIAL: Mutex<()> = Mutex::new(());

    fn serial() -> MutexGuard<'static, ()> {
        TEST_SERIAL.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// 테스트용 임시 lifetime — 파일 영속화를 건드리지 않도록 항상 Temporary.
    fn temp(limit: Limit) -> Lifetime {
        Lifetime {
            persistence: Persistence::Temporary,
            limit,
        }
    }

    /// poison 이 걸린 뒤에도 레지스트리가 읽고 쓰이는가.
    ///
    /// 전역([`STATE`])이 아니라 지역 뮤텍스를 겨냥한다 — 전역을 poison 하면 같은 테스트
    /// 바이너리의 뒤 테스트가 그 상태를 물려받는다.
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
            "복구했으면 한 번은 보고해야 한다 — 조용한 복구는 조용한 유실과 구분되지 않는다"
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

        // 매칭: 올바른 메서드 → Matched, 틀린 메서드 → MethodNotAllowed.
        assert!(matches!(
            match_request(&out.id, "POST"),
            MatchResult::Matched { .. }
        ));
        assert!(matches!(
            match_request(&out.id, "GET"),
            MatchResult::MethodNotAllowed
        ));
        assert!(matches!(
            match_request("nope", "POST"),
            MatchResult::NotFound
        ));

        assert!(unregister(&out.id));
        assert!(info(&out.id).is_none());
        // 해제 후 404.
        assert!(matches!(
            match_request(&out.id, "POST"),
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
        // 순차 카운터가 아님(랜덤) — 인접 등록이 인접 id 를 주지 않는다.
        unregister(&a.id);
        unregister(&b.id);
    }

    #[test]
    fn count_limit_consumes_and_self_destructs() {
        let _g = serial();
        // 횟수제한 N=2 → 2 호출 성공, 3번째는 소멸(404).
        let out = register(
            vec!["POST".to_string()],
            None,
            vec![],
            temp(Limit::CountLimit { remaining: 2 }),
            None,
        );
        assert!(matches!(
            match_request(&out.id, "POST"),
            MatchResult::Matched { .. }
        ));
        // 1회 소비 후에도 info 로 남은 카운트 확인.
        assert!(matches!(
            info(&out.id).unwrap().0.lifetime.limit,
            Limit::CountLimit { remaining: 1 }
        ));
        assert!(matches!(
            match_request(&out.id, "POST"),
            MatchResult::Matched { .. }
        ));
        // 2회 소진 → 엔트리 삭제 → 3번째는 NotFound(404).
        assert!(info(&out.id).is_none());
        assert!(matches!(
            match_request(&out.id, "POST"),
            MatchResult::NotFound
        ));
    }

    #[test]
    fn time_limit_lazy_expires_with_410() {
        let _g = serial();
        // 이미 지난 deadline → 첫 호출에서 Expired(410) + 삭제.
        let out = register(
            vec!["POST".to_string()],
            None,
            vec![],
            temp(Limit::TimeLimit { deadline_unix: 1 }),
            None,
        );
        assert!(matches!(
            match_request(&out.id, "POST"),
            MatchResult::Expired
        ));
        // 만료 응답과 함께 삭제됨 → 이후 404.
        assert!(info(&out.id).is_none());
        assert!(matches!(
            match_request(&out.id, "POST"),
            MatchResult::NotFound
        ));
    }

    #[test]
    fn method_mismatch_does_not_consume_count() {
        let _g = serial();
        // 매칭 실패(405)는 카운트를 차감하지 않는다("매칭 성공 시" 규칙).
        let out = register(
            vec!["POST".to_string()],
            None,
            vec![],
            temp(Limit::CountLimit { remaining: 1 }),
            None,
        );
        assert!(matches!(
            match_request(&out.id, "GET"),
            MatchResult::MethodNotAllowed
        ));
        // 여전히 remaining=1.
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
