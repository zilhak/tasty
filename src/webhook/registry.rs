//! 웹훅 등록 상태 — 프로세스 전역 싱글턴.
//!
//! 리스너 thread(요청 매칭)와 IPC 핸들러 thread(register/list/info/unregister)가
//! 같은 상태를 공유하므로 `OnceLock<Mutex<..>>` 로 둔다. MVP lifetime 은
//! Temporary/Unlimited 고정 — 영속화·횟수/시간 제한은 후속(S5).

use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard, OnceLock};

use crate::adapters::ipc::host_call::HostIpcInjector;
use crate::hook_handler::{HookHandlerId, IpcCall};

/// 등록된 웹훅 엔트리 (MVP).
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

fn lock() -> MutexGuard<'static, WebhookState> {
    state().lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// 리스너 runtime 설정 주입(부팅 헬퍼가 bind 전에 호출).
pub(super) fn set_runtime(injector: HostIpcInjector, bind_addr: &str, port: u16) {
    let mut s = lock();
    s.injector = Some(injector);
    s.bind_addr = bind_addr.to_string();
    s.port = Some(port);
}

/// 이미 bind 되었는지(부팅 헬퍼 중복 호출 가드).
pub(super) fn is_bound() -> bool {
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

/// 웹훅 등록. opaque id 를 발급하고 (id, 메서드, 핸들러, 시퀀스) 를 저장한다.
/// 발급 URL 을 반환.
pub fn register(
    methods: Vec<String>,
    handler_id: Option<HookHandlerId>,
    calls: Vec<IpcCall>,
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
        },
    );
    RegisterOutcome { id, url }
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

/// 웹훅 해제. 존재했으면 `true`.
pub fn unregister(id: &str) -> bool {
    lock().entries.remove(id).is_some()
}

/// 리스너 thread 의 요청 매칭 결과.
pub(super) enum MatchResult {
    NotFound,
    MethodNotAllowed,
    Matched {
        calls: Vec<IpcCall>,
        injector: Option<HostIpcInjector>,
    },
}

/// (path, method) 로 매칭해 실행할 시퀀스 스냅샷 + injector 를 반환한다.
/// 짧게 lock 을 잡고 clone 만 떠서 나온다(실행은 lock 밖).
pub(super) fn match_request(path: &str, method: &str) -> MatchResult {
    let s = lock();
    match s.entries.get(path) {
        None => MatchResult::NotFound,
        Some(entry) => {
            if entry.methods.iter().any(|m| m == method) {
                MatchResult::Matched {
                    calls: entry.calls.clone(),
                    injector: s.injector.clone(),
                }
            } else {
                MatchResult::MethodNotAllowed
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_list_info_unregister_roundtrip() {
        let calls = vec![IpcCall {
            method: "notification.create".to_string(),
            params: serde_json::json!({"body": "${body.message}"}),
        }];
        let out = register(vec!["POST".to_string()], None, calls.clone());
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
        assert!(matches!(match_request("nope", "POST"), MatchResult::NotFound));

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
        let a = register(vec!["POST".to_string()], None, vec![]);
        let b = register(vec!["POST".to_string()], None, vec![]);
        assert_ne!(a.id, b.id);
        // 순차 카운터가 아님(랜덤) — 인접 등록이 인접 id 를 주지 않는다.
        unregister(&a.id);
        unregister(&b.id);
    }
}
