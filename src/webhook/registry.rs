//! 리스너와 IPC 핸들러가 공유하는 웹훅 등록 목록.
//! Mutex로 접근을 직렬화하며 만료는 요청·복원·sweep에서 확인한다.

mod mutation;
mod ordered;
mod receipt;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};

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

use crate::app::file_writer::{FileExecutor, command};
use mutation::Operation;
pub(super) use receipt::Lease;
use receipt::{Budget, Reply};
pub(crate) use receipt::{Completed, Failure, Receipt, Value};
use std::{
    collections::{HashSet, VecDeque},
    path::PathBuf,
};
use tasty_utils::batch_writer::{Handle, Limits};

struct Entry {
    value: WebhookEntry,
    identity: Arc<()>,
}
impl std::ops::Deref for Entry {
    type Target = WebhookEntry;
    fn deref(&self) -> &WebhookEntry {
        &self.value
    }
}

#[derive(Default)]
struct State {
    entries: BTreeMap<String, Entry>,
    reserved: HashSet<String>,
    ports: Option<Arc<crate::runtime_ports::RuntimePorts>>,
    injector: Option<HostIpcInjector>,
    bound: bool,
    closing: bool,
    uncertain: bool,
    booting: bool,
    publishing: bool,
    dirty: bool,
    unsaved_reported: bool,
    accepted: u64,
    rejected: u64,
    settled: u64,
    pending: VecDeque<ordered::Pending>,
    in_flight: Option<ordered::InFlight>,
    after_boot: Option<Box<dyn FnOnce() + Send>>,
    listener: Option<super::listener::Listener>,
}

/// One process-owned ordered coordinator. Only the existing file worker performs writes.
pub(crate) struct Registry {
    state: Mutex<State>,
    queue: Option<Handle<FileExecutor>>,
    path: PathBuf,
    budget: Arc<Budget>,
    #[cfg(test)]
    panic_after_replace: std::sync::atomic::AtomicBool,
}
const STATE_WHAT: &str = "the webhook registry";
static STATE_POISON_REPORTED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
fn lock_state(state: &Mutex<State>) -> MutexGuard<'_, State> {
    tasty_utils::poison::recover_mutex(state.lock(), STATE_WHAT, &STATE_POISON_REPORTED)
}

impl Registry {
    pub(crate) fn new(queue: Option<Handle<FileExecutor>>, path: PathBuf) -> Self {
        Self::with_limits(queue, path, Limits::default())
    }
    fn with_limits(queue: Option<Handle<FileExecutor>>, path: PathBuf, limits: Limits) -> Self {
        Self {
            state: Mutex::new(State::default()),
            queue,
            path,
            budget: Budget::new(limits),
            #[cfg(test)]
            panic_after_replace: std::sync::atomic::AtomicBool::new(false),
        }
    }
    fn lock(&self) -> MutexGuard<'_, State> {
        lock_state(&self.state)
    }
    pub(super) fn set_runtime(
        &self,
        injector: HostIpcInjector,
        ports: Arc<crate::runtime_ports::RuntimePorts>,
    ) {
        let mut s = self.lock();
        s.injector = Some(injector);
        s.ports = Some(ports);
    }
    pub(super) fn is_bound(&self) -> bool {
        self.lock().bound
    }
    pub(super) fn install_listener(&self, listener: super::listener::Listener) {
        let mut s = self.lock();
        s.bound = true;
        s.listener = Some(listener);
    }
    pub(super) fn path(&self) -> &std::path::Path {
        &self.path
    }
    pub(super) fn begin_restore(&self) {
        self.lock().booting = true;
    }
    pub(super) fn after_restore(&self, ready: impl FnOnce() + Send + 'static) {
        self.lock().after_boot = Some(Box::new(ready));
        self.poll();
    }
    pub(super) fn restore_entry(&self, entry: WebhookEntry) {
        let mut s = self.lock();
        s.entries.entry(entry.id.clone()).or_insert(Entry {
            value: entry,
            identity: Arc::new(()),
        });
    }
    pub(crate) fn list(&self) -> Vec<(WebhookEntry, String)> {
        let s = self.lock();
        s.entries
            .values()
            .map(|e| (e.value.clone(), build_url(&s, &e.id)))
            .collect()
    }
    pub(crate) fn info(&self, id: &str) -> Option<(WebhookEntry, String)> {
        let s = self.lock();
        s.entries
            .get(id)
            .map(|e| (e.value.clone(), build_url(&s, &e.id)))
    }
    pub(crate) fn register(
        &self,
        methods: Vec<String>,
        handler_id: Option<HookHandlerId>,
        calls: Vec<IpcCall>,
        lifetime: Lifetime,
        auth: Option<WebhookAuth>,
    ) -> Receipt {
        let mut s = self.lock();
        let id = gen_opaque_id(&s);
        let entry = WebhookEntry {
            id: id.clone(),
            methods,
            handler_id,
            calls,
            lifetime,
            auth,
        };
        let result = self.admit(&mut s, Operation::Register(entry), false, 0);
        drop(s);
        self.poll();
        result
    }
    pub(crate) fn unregister(&self, id: &str) -> Receipt {
        self.submit(Operation::Remove(id.to_owned()), false, 0)
    }
    pub(crate) fn sweep(&self) -> Receipt {
        self.submit(Operation::Sweep, false, 0)
    }
    pub(super) fn persist_now(&self) {
        let mut receipt = self.admit(&mut self.lock(), Operation::Persist, true, 0);
        if let Some(Err(error)) = receipt.poll() {
            self.lock().dirty = true;
            tracing::warn!(error=%error.message,"webhook rewrite was not admitted");
            crate::hook_runtime::notice::webhook_save_failed(&error.message);
        }
        self.poll();
    }
    fn submit(&self, operation: Operation, internal: bool, context_bytes: usize) -> Receipt {
        let result = self.admit(&mut self.lock(), operation, internal, context_bytes);
        self.poll();
        result
    }
    fn admit(
        &self,
        s: &mut State,
        operation: Operation,
        internal: bool,
        context_bytes: usize,
    ) -> Receipt {
        if s.closing || s.uncertain || s.booting && !internal {
            s.rejected = s.rejected.saturating_add(1);
            return Receipt::refused(Failure::refused(
                "webhook registry is not accepting mutations",
            ));
        }
        let bytes = operation
            .bytes()
            .saturating_mul(2)
            .saturating_add(context_bytes)
            .saturating_add(256);
        let lease = match self.budget.admit(bytes) {
            Ok(lease) => lease,
            Err(e) => {
                s.rejected = s.rejected.saturating_add(1);
                return Receipt::refused(e);
            }
        };
        s.accepted = s.accepted.saturating_add(1);
        if let Operation::Register(entry) = &operation {
            s.reserved.insert(entry.id.clone());
        }
        let (receipt, reply) = Receipt::new(lease);
        s.pending.push_back(ordered::Pending { operation, reply });
        receipt
    }
    /// Authentication never enters the file request; identity remains stable across consumption.
    pub(super) fn match_request(
        &self,
        path: &str,
        method: &str,
        context_bytes: usize,
        authorized: impl FnOnce(&WebhookAuth) -> bool,
    ) -> (MatchResult, Option<Lease>) {
        let evidence = {
            let s = self.lock();
            s.entries
                .get(path)
                .map(|e| (e.identity.clone(), e.auth.as_ref().is_none_or(authorized)))
        };
        let receipt = self.submit(
            Operation::Match {
                id: path.into(),
                method: method.into(),
                evidence,
            },
            false,
            context_bytes,
        );
        let (result, lease) = receipt.wait();
        match result {
            Ok(Completed {
                value: Value::Matched(result),
                ..
            }) => (result, lease),
            _ => (MatchResult::Unavailable, lease),
        }
    }
    /// Close producers before the common writer closes admission; no waiting or disk I/O.
    pub(crate) fn status(&self) -> Status {
        let s = self.lock();
        let (retained_items, retained_bytes) = self.budget.usage();
        Status {
            accepted: s.accepted,
            rejected: s.rejected,
            settled: s.settled,
            pending: s.pending.len() + usize::from(s.in_flight.is_some()),
            retained_items,
            retained_bytes,
            closing: s.closing,
            dirty: s.dirty,
            outcome_unknown: s.uncertain,
        }
    }
    pub(crate) fn begin_shutdown(&self) {
        let (listener, boot) = {
            let mut s = self.lock();
            s.closing = true;
            (s.listener.take(), s.after_boot.take())
        };
        drop(boot);
        if let Some(listener) = listener {
            listener.stop();
            self.lock().listener = Some(listener);
        }
    }
    pub(crate) fn poll_shutdown(&self) -> usize {
        self.begin_shutdown();
        self.poll();
        let listener = self.lock().listener.take();
        let mut waiting = 0;
        if let Some(mut listener) = listener
            && !listener.poll_stopped()
        {
            waiting = 1;
            self.lock().listener = Some(listener);
        }
        let pending = usize::from(self.budget.usage().0 != 0 || waiting != 0);
        let warn = {
            let mut s = self.lock();
            let warn = pending == 0 && (s.dirty || s.uncertain) && !s.unsaved_reported;
            if warn {
                s.unsaved_reported = true;
            }
            warn
        };
        if warn {
            tracing::warn!(
                "webhook producers retired with unsaved or unknown registry changes; not a clean save"
            );
        }
        pending
    }
}

#[derive(serde::Serialize)]
pub(crate) struct Status {
    pub(crate) accepted: u64,
    pub(crate) rejected: u64,
    pub(crate) settled: u64,
    pub(crate) pending: usize,
    pub(crate) retained_items: usize,
    pub(crate) retained_bytes: usize,
    pub(crate) closing: bool,
    pub(crate) dirty: bool,
    pub(crate) outcome_unknown: bool,
}

fn gen_opaque_id(s: &State) -> String {
    use rand::Rng;
    let mut rng = rand::rng();
    loop {
        let bytes: [u8; 8] = rng.random();
        let id: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        if !s.entries.contains_key(&id) && !s.reserved.contains(&id) {
            return id;
        }
    }
}
fn build_url(s: &State, id: &str) -> String {
    let addr = s.ports.as_ref().and_then(|p| p.webhook()).map(|b| b.addr);
    match addr {
        Some(addr) => {
            let host = if addr.ip().is_unspecified() {
                std::net::IpAddr::from([127, 0, 0, 1])
            } else {
                addr.ip()
            };
            format!("http://{host}:{}/{id}", addr.port())
        }
        None => format!("http://127.0.0.1/{id}"),
    }
}
#[derive(Debug, Clone)]
pub struct RegisterOutcome {
    pub id: String,
    pub url: String,
}

pub(crate) enum MatchResult {
    NotFound,
    MethodNotAllowed,
    Expired,
    Unauthorized,
    Unavailable,
    Matched {
        calls: Vec<IpcCall>,
        injector: Option<HostIpcInjector>,
    },
}

#[cfg(test)]
mod tests;
