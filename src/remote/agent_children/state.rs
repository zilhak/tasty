//! 서버 쪽 원격 출처·실행 문맥·call 결과·자식 기록. 엔진·plugin 없이 시험할 수 있는 부분만 둔다.
//!
//! 출처(origin)는 holder 연결에 묶인 비공개 토큰으로 정한다. 같은 토큰을 다시 제시한 연결이 그
//! 출처를 이어받는다. 토큰은 서버 프로세스 메모리에만 있어 서버가 다시 시작하면 무효다.
//! 서버 child registry 에는 공개 번호로 만든 부모 키(`attach:o<n>`)만 남는다.

use std::collections::{BTreeSet, HashMap};
use std::time::{Duration, Instant};

use tasty_ipc::agent_child::{
    AckOutcome, AgentServerMsg, CallError, CallOutcome, Notice, PROCESS_EXIT, parent_key,
};

use super::outbox::Outbox;

/// 끝난 call 의 결과를 같은 call_id 재요청에 돌려주기 위해 보관하는 기간.
pub(super) const CALL_TTL: Duration = Duration::from_secs(10 * 60);
/// 한 연결에서 동시에 진행할 수 있는 call 수.
pub(super) const MAX_RUNNING_PER_CLIENT: usize = 8;

/// plugin 호출 하나 동안 그 plugin 이 holder 대신 쓸 수 있는 문맥.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Context {
    pub origin: u64,
    pub client: u32,
    pub workspace: u32,
    pub plugin: String,
}

/// 자식을 만든 plugin 의 선언. 통지로 옮길 수 있는 이벤트·메타 키를 정한다.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Decl {
    pub namespace: String,
    pub hook_events: Vec<String>,
    pub relay_meta: Vec<String>,
}

impl Decl {
    /// host 가 만드는 종료 이벤트와 State 는 선언 없이 옮긴다. plugin 결과는 그 plugin 자신의 것만이다.
    pub(crate) fn allows(&self, notice: &Notice) -> bool {
        match notice {
            Notice::State { .. } => true,
            Notice::Event { name } => name == PROCESS_EXIT || self.hook_events.contains(name),
            Notice::Meta { key, .. } => self.relay_meta.contains(key),
            Notice::Plugin { namespace, .. } => *namespace == self.namespace,
        }
    }
}

pub(super) struct Call {
    pub origin: u64,
    pub plugin: String,
    pub context: String,
    /// call 시작 때 이 출처의 자식 surface. 끝날 때의 목록과 비교해 새 자식을 찾는다.
    pub before: BTreeSet<u32>,
    pub created: Instant,
    pub result: Option<AgentServerMsg>,
}

/// 출처의 자식. call 결과로 알게 되고 client 의 commit 뒤에만 통지를 보낸다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Child {
    pub origin: u64,
    pub plugin: String,
    pub committed: bool,
}

pub(crate) enum Begin {
    /// 같은 call_id 의 결과가 이미 있다. 다시 실행하지 않고 그 결과를 보낸다.
    Cached(AgentServerMsg),
    /// 같은 call_id 가 진행 중이다. 끝나면 지금 연결에 보낸다.
    Running,
    /// 새로 실행한다. `context` 를 plugin 호출에 싣는다.
    Start {
        origin: u64,
        context: String,
    },
    Rejected(CallError),
}

pub(crate) struct Server {
    pub(super) outbox: Outbox,
    next_origin: u64,
    tokens: HashMap<String, u64>,
    bound: HashMap<u32, u64>,
    contexts: HashMap<String, Context>,
    pub(super) calls: HashMap<(u64, u64), Call>,
    pub(super) children: HashMap<u32, Child>,
    /// plugin 이 준 결과 key → outbox key. plugin 은 확인될 때까지 같은 key 로 다시 묻는다.
    plugin_keys: HashMap<(String, String), String>,
    /// client 가 확인한 outbox key 와 결과. 오래된 것부터 버린다.
    settled: std::collections::VecDeque<(String, AckOutcome)>,
    /// plugin id → 그 plugin 의 원격 선언. call 을 시작할 때 매니페스트에서 갱신한다.
    decls: HashMap<String, Decl>,
}

/// 확인 결과를 기억하는 수. plugin 재시도 간격 안에 다시 묻는 데 충분하다.
const SETTLED_CAP: usize = 4096;

/// 통지가 속한 출처·서버 workspace·원격 surface.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Placed {
    pub origin: u64,
    pub workspace: u32,
    pub surface: u32,
}

/// plugin 결과 통지의 처리 상태.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PluginNoticeStatus {
    /// 아직 client 가 확인하지 않았다. plugin 은 영속 기록을 지우지 않는다.
    Pending,
    /// client 가 기록했거나 이미 기록한 결과다.
    Delivered,
    /// client 가 처리할 수 없다고 답했다.
    Rejected(String),
}

fn random_hex(bytes: usize) -> String {
    use rand::RngCore;
    let mut buf = vec![0u8; bytes];
    rand::rng().fill_bytes(&mut buf);
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

pub(crate) fn error(code: i32, message: impl Into<String>) -> CallError {
    CallError {
        code,
        message: message.into(),
    }
}

impl Server {
    pub(crate) fn new() -> Self {
        Self {
            outbox: Outbox::new(random_hex(6)),
            next_origin: 0,
            tokens: HashMap::new(),
            bound: HashMap::new(),
            contexts: HashMap::new(),
            calls: HashMap::new(),
            children: HashMap::new(),
            plugin_keys: HashMap::new(),
            settled: std::collections::VecDeque::new(),
            decls: HashMap::new(),
        }
    }

    /// 연결의 출처를 정한다. 아는 토큰이면 그 출처를 이 연결로 옮기고, 아니면 새로 만든다.
    pub(crate) fn hello(
        &mut self,
        client: u32,
        token: Option<&str>,
        now: Instant,
    ) -> AgentServerMsg {
        let known = token.and_then(|token| self.tokens.get(token).map(|o| (token.to_owned(), *o)));
        let resumed = known.is_some();
        let (token, origin) = known.unwrap_or_else(|| {
            self.next_origin += 1;
            let token = random_hex(16);
            self.tokens.insert(token.clone(), self.next_origin);
            (token, self.next_origin)
        });
        let previous: Vec<u32> = self
            .bound
            .iter()
            .filter(|(c, o)| **o == origin && **c != client)
            .map(|(c, _)| *c)
            .collect();
        for old in previous {
            self.bound.remove(&old);
            self.outbox.unsent_for(old, now);
        }
        self.bound.insert(client, origin);
        AgentServerMsg::AgentHelloAck {
            origin_token: token,
            origin,
            resumed,
        }
    }

    pub(crate) fn origin_of_client(&self, client: u32) -> Option<u64> {
        self.bound.get(&client).copied()
    }

    pub(crate) fn client_of_origin(&self, origin: u64) -> Option<u32> {
        self.bound
            .iter()
            .find(|(_, o)| **o == origin)
            .map(|(c, _)| *c)
    }

    /// 끊긴 연결의 출처 묶음과 문맥을 지운다. 보낸 통지는 다음 연결에 다시 보낸다.
    pub(crate) fn disconnect(&mut self, client: u32, now: Instant) {
        self.bound.remove(&client);
        self.contexts.retain(|_, c| c.client != client);
        self.outbox.unsent_for(client, now);
    }

    pub(crate) fn set_decl(&mut self, plugin: &str, decl: Decl) {
        self.decls.insert(plugin.to_owned(), decl);
    }

    pub(crate) fn decl(&self, plugin: &str) -> Option<&Decl> {
        self.decls.get(plugin)
    }

    /// 이 자식의 통지를 정할 plugin. 기록된 자식이면 만든 plugin, 아직이면 그 출처의 진행 중 call 의 plugin 들이다.
    pub(crate) fn notice_allowed(&self, origin: u64, surface: u32, notice: &Notice) -> bool {
        let allowed = |plugin: &str| self.decls.get(plugin).is_some_and(|d| d.allows(notice));
        match self.children.get(&surface) {
            Some(child) => child.origin == origin && allowed(&child.plugin),
            None => self
                .contexts
                .values()
                .any(|c| c.origin == origin && allowed(&c.plugin)),
        }
    }

    /// 이 출처에서 이 call 을 끝까지 처리할 대상인지. 서버가 재시작해 모르는 출처의 call 은 없다.
    pub(crate) fn is_child_of(&self, origin: u64, surface: u32) -> bool {
        self.children
            .get(&surface)
            .is_some_and(|c| c.origin == origin)
    }

    pub(crate) fn begin_call(
        &mut self,
        client: u32,
        call_id: u64,
        plugin: &str,
        workspace: u32,
        before: BTreeSet<u32>,
        now: Instant,
    ) -> Begin {
        let Some(origin) = self.origin_of_client(client) else {
            return Begin::Rejected(error(-32600, "agent_hello is required before calls"));
        };
        if let Some(call) = self.calls.get(&(origin, call_id)) {
            return match &call.result {
                Some(result) => Begin::Cached(result.clone()),
                None => Begin::Running,
            };
        }
        let running = self
            .calls
            .values()
            .filter(|c| c.origin == origin && c.result.is_none())
            .count();
        if running >= MAX_RUNNING_PER_CLIENT {
            return Begin::Rejected(error(-32005, "too many remote agent calls in progress"));
        }
        let context = random_hex(16);
        self.contexts.insert(
            context.clone(),
            Context {
                origin,
                client,
                workspace,
                plugin: plugin.to_owned(),
            },
        );
        self.calls.insert(
            (origin, call_id),
            Call {
                origin,
                plugin: plugin.to_owned(),
                context: context.clone(),
                before,
                created: now,
                result: None,
            },
        );
        Begin::Start { origin, context }
    }

    /// call 이 끝났다. 새 자식을 기록하고 문맥을 폐기한다. 보낼 결과를 돌려준다.
    pub(crate) fn finish_call(
        &mut self,
        origin: u64,
        call_id: u64,
        after: &BTreeSet<u32>,
        response: Result<serde_json::Value, CallError>,
    ) -> Option<AgentServerMsg> {
        let call = self.calls.get_mut(&(origin, call_id))?;
        self.contexts.remove(&call.context);
        let spawned: Vec<u32> = after.difference(&call.before).copied().collect();
        for surface in &spawned {
            self.children.entry(*surface).or_insert_with(|| Child {
                origin,
                plugin: call.plugin.clone(),
                committed: false,
            });
        }
        let outcome = match response {
            Ok(result) => CallOutcome::Ok { result },
            Err(error) => CallOutcome::Error(error),
        };
        let msg = AgentServerMsg::AgentCallResult {
            call_id,
            outcome,
            spawned,
        };
        call.result = Some(msg.clone());
        Some(msg)
    }

    /// client 가 부모 경로를 준비했다. 그 call 로 생긴 자식만 열 수 있다.
    pub(crate) fn commit(&mut self, client: u32, call_id: u64, surfaces: &[u32]) -> usize {
        let Some(origin) = self.origin_of_client(client) else {
            return 0;
        };
        let Some(AgentServerMsg::AgentCallResult { spawned, .. }) = self
            .calls
            .get(&(origin, call_id))
            .and_then(|c| c.result.as_ref())
        else {
            return 0;
        };
        let spawned = spawned.clone();
        let mut opened = 0;
        for surface in surfaces.iter().filter(|s| spawned.contains(s)) {
            if let Some(child) = self.children.get_mut(surface)
                && child.origin == origin
            {
                child.committed = true;
                opened += 1;
            }
        }
        opened
    }

    pub(crate) fn context(&self, token: &str) -> Option<&Context> {
        self.contexts.get(token)
    }

    /// 이 출처의 자식에 이 plugin 이 holder 대신 입력·종료할 수 있는지. 출처가 지금 `holder` 연결에
    /// 묶여 있어야 한다. 자식을 만든 plugin 이거나 그 출처의 진행 중 call 을 처리하는 plugin 이다.
    pub(crate) fn plugin_may_drive(
        &self,
        origin: u64,
        surface: u32,
        plugin: &str,
        holder: u32,
    ) -> bool {
        if self.origin_of_client(holder) != Some(origin) {
            return false;
        }
        match self.children.get(&surface) {
            Some(child) => child.origin == origin && child.plugin == plugin,
            None => self
                .contexts
                .values()
                .any(|c| c.origin == origin && c.client == holder && c.plugin == plugin),
        }
    }

    /// 원격 출처 자식의 일을 통지로 쌓는다.
    pub(crate) fn relay(
        &mut self,
        origin: u64,
        workspace: u32,
        surface: u32,
        notice: Notice,
        now: Instant,
    ) {
        self.outbox.push(origin, workspace, surface, notice, now);
    }

    /// 보낼 통지. commit 된 자식만, 출처가 묶인 연결이 그 workspace 의 holder 일 때만 보낸다.
    pub(crate) fn due(
        &mut self,
        now: Instant,
        holder_of: impl Fn(u32) -> Option<u32>,
    ) -> Vec<(u32, AgentServerMsg)> {
        let children = &self.children;
        let bound = &self.bound;
        self.outbox.due(now, |entry| {
            if !children
                .get(&entry.surface)
                .is_some_and(|c| c.committed && c.origin == entry.origin)
            {
                return None;
            }
            let client = holder_of(entry.workspace)?;
            (bound.get(&client) == Some(&entry.origin)).then_some(client)
        })
    }

    pub(crate) fn ack(
        &mut self,
        client: u32,
        key: &str,
        outcome: &AckOutcome,
        now: Instant,
    ) -> bool {
        let Some(origin) = self.origin_of_client(client) else {
            return false;
        };
        let applied = self.outbox.ack(origin, key, outcome, now);
        if applied && outcome.settles() {
            if self.settled.len() >= SETTLED_CAP {
                self.settled.pop_front();
            }
            self.settled.push_back((key.to_owned(), outcome.clone()));
        }
        applied
    }

    /// plugin 고유 결과를 보낸다. 같은 (plugin, key) 를 다시 부르면 새로 쌓지 않고 상태만 돌려준다.
    pub(crate) fn plugin_notice(
        &mut self,
        plugin: &str,
        key: &str,
        at: Placed,
        notice: Notice,
        now: Instant,
    ) -> PluginNoticeStatus {
        let Placed {
            origin,
            workspace,
            surface,
        } = at;
        let id = (plugin.to_owned(), key.to_owned());
        if let Some(outbox_key) = self.plugin_keys.get(&id) {
            if self.outbox.holds_key(outbox_key) {
                return PluginNoticeStatus::Pending;
            }
            if let Some((_, outcome)) = self.settled.iter().find(|(k, _)| k == outbox_key) {
                return match outcome {
                    AckOutcome::Rejected { reason } => PluginNoticeStatus::Rejected(reason.clone()),
                    _ => PluginNoticeStatus::Delivered,
                };
            }
            // 보관 기간이 지나 버렸거나 기록을 잃었다. 다시 쌓는다.
        }
        let outbox_key = self.outbox.push(origin, workspace, surface, notice, now);
        self.plugin_keys.insert(id, outbox_key);
        PluginNoticeStatus::Pending
    }

    /// 오래된 call 결과·통지를 정리하고, 사라졌고 남은 통지도 없는 자식 기록을 지운다.
    pub(crate) fn expire(&mut self, now: Instant, live: impl Fn(u32) -> bool) {
        self.calls
            .retain(|_, c| c.result.is_none() || now.duration_since(c.created) < CALL_TTL);
        self.outbox.expire(now);
        let outbox = &self.outbox;
        self.children
            .retain(|surface, _| live(*surface) || outbox.holds_surface(*surface));
        let settled = &self.settled;
        self.plugin_keys
            .retain(|_, k| outbox.holds_key(k) || settled.iter().any(|(s, _)| s == k));
    }

    /// 출처 번호의 공개 부모 키.
    pub(crate) fn parent_key(origin: u64) -> String {
        parent_key(origin)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plugin_result_stays_pending_until_the_client_confirms_it() {
        let now = Instant::now();
        let mut s = Server::new();
        let (_, o) = token(&s.hello(1, None, now));
        s.begin_call(1, 7, "p", 4, BTreeSet::new(), now);
        s.finish_call(o, 7, &BTreeSet::from([9]), Ok(serde_json::json!({})));
        s.commit(1, 7, &[9]);
        let at = |origin| Placed {
            origin,
            workspace: 4,
            surface: 9,
        };
        let payload = || Notice::Plugin {
            namespace: "p".into(),
            payload: serde_json::json!({"k": 1}),
        };
        assert_eq!(
            s.plugin_notice("p", "c1", at(o), payload(), now),
            PluginNoticeStatus::Pending
        );
        // 재시도는 새 통지를 쌓지 않는다.
        assert_eq!(
            s.plugin_notice("p", "c1", at(o), payload(), now),
            PluginNoticeStatus::Pending
        );
        let sent = s.due(now, |_| Some(1));
        assert_eq!(sent.len(), 1);
        let AgentServerMsg::ChildNotice { key, .. } = &sent[0].1 else {
            panic!()
        };
        s.ack(1, key, &AckOutcome::Duplicate, now);
        assert_eq!(
            s.plugin_notice("p", "c1", at(o), payload(), now),
            PluginNoticeStatus::Delivered
        );
        assert!(
            s.due(now + super::super::outbox::RESEND_AFTER, |_| Some(1))
                .is_empty()
        );
    }

    fn token(msg: &AgentServerMsg) -> (String, u64) {
        match msg {
            AgentServerMsg::AgentHelloAck {
                origin_token,
                origin,
                ..
            } => (origin_token.clone(), *origin),
            other => panic!("{other:?}"),
        }
    }

    fn idle() -> Notice {
        Notice::Event { name: "e".into() }
    }

    #[test]
    fn a_reconnect_with_the_token_keeps_the_origin_and_another_token_does_not() {
        let now = Instant::now();
        let mut s = Server::new();
        let (t, o) = token(&s.hello(1, None, now));
        assert_eq!(token(&s.hello(2, Some(&t), now)).1, o);
        assert_eq!(s.origin_of_client(1), None);
        assert_eq!(s.client_of_origin(o), Some(2));
        assert!(matches!(
            s.hello(4, Some(&t), now),
            AgentServerMsg::AgentHelloAck { resumed: true, .. }
        ));
        let unknown = s.hello(3, Some("guess"), now);
        assert!(matches!(
            unknown,
            AgentServerMsg::AgentHelloAck { resumed: false, .. }
        ));
        assert_ne!(token(&unknown).1, o);
    }

    #[test]
    fn notices_wait_for_commit_and_go_only_to_the_bound_holder() {
        let now = Instant::now();
        let mut s = Server::new();
        let (_, o) = token(&s.hello(1, None, now));
        let Begin::Start { .. } = s.begin_call(1, 7, "p", 4, BTreeSet::new(), now) else {
            panic!()
        };
        let after = BTreeSet::from([9]);
        let msg = s
            .finish_call(o, 7, &after, Ok(serde_json::json!({})))
            .unwrap();
        assert!(
            matches!(msg, AgentServerMsg::AgentCallResult { ref spawned, .. } if spawned == &vec![9])
        );
        s.relay(o, 4, 9, idle(), now);
        // commit 전에는 보내지 않는다(부모 경로가 준비되기 전 완료 유실 방지).
        assert!(s.due(now, |_| Some(1)).is_empty());
        // 그 call 로 생기지 않은 surface 는 열 수 없다.
        assert_eq!(s.commit(1, 7, &[10]), 0);
        assert_eq!(s.commit(1, 7, &[9]), 1);
        // holder 가 다른 연결이면 보내지 않는다.
        assert!(s.due(now, |_| Some(5)).is_empty());
        assert_eq!(s.due(now, |_| Some(1)).len(), 1);
    }

    #[test]
    fn the_same_call_id_returns_the_kept_result_without_running_again() {
        let now = Instant::now();
        let mut s = Server::new();
        let (t, o) = token(&s.hello(1, None, now));
        s.begin_call(1, 7, "p", 4, BTreeSet::new(), now);
        assert!(matches!(
            s.begin_call(1, 7, "p", 4, BTreeSet::new(), now),
            Begin::Running
        ));
        s.finish_call(o, 7, &BTreeSet::new(), Err(error(1, "x")));
        s.disconnect(1, now);
        s.hello(2, Some(&t), now);
        assert!(matches!(
            s.begin_call(2, 7, "p", 4, BTreeSet::new(), now),
            Begin::Cached(AgentServerMsg::AgentCallResult {
                outcome: CallOutcome::Error(_),
                ..
            })
        ));
        s.expire(now + CALL_TTL, |_| false);
        assert!(matches!(
            s.begin_call(2, 7, "p", 4, BTreeSet::new(), now),
            Begin::Start { .. }
        ));
    }

    #[test]
    fn only_the_creating_plugin_on_the_bound_holder_may_drive_a_child() {
        let now = Instant::now();
        let mut s = Server::new();
        let (_, o) = token(&s.hello(1, None, now));
        let Begin::Start { context, .. } = s.begin_call(1, 7, "p", 4, BTreeSet::new(), now) else {
            panic!()
        };
        // 진행 중 call 의 plugin 은 아직 기록되지 않은 새 자식에 쓸 수 있다.
        assert!(s.plugin_may_drive(o, 9, "p", 1));
        assert!(!s.plugin_may_drive(o, 9, "q", 1));
        assert_eq!(s.context(&context).map(|c| c.workspace), Some(4));
        s.finish_call(o, 7, &BTreeSet::from([9]), Ok(serde_json::json!({})));
        assert!(s.context(&context).is_none());
        assert!(s.plugin_may_drive(o, 9, "p", 1));
        assert!(!s.plugin_may_drive(o, 9, "q", 1));
        // 출처가 다른 연결로 옮겨 가면 이전 holder 로는 쓸 수 없다.
        assert!(!s.plugin_may_drive(o, 9, "p", 2));
        s.disconnect(1, now);
        assert!(!s.plugin_may_drive(o, 9, "p", 1));
    }

    #[test]
    fn only_declared_events_and_meta_of_the_creating_plugin_are_relayed() {
        let now = Instant::now();
        let mut s = Server::new();
        let (_, o) = token(&s.hello(1, None, now));
        s.set_decl(
            "p",
            Decl {
                namespace: "p".into(),
                hook_events: vec!["p-idle".into()],
                relay_meta: vec!["p-note".into()],
            },
        );
        s.begin_call(1, 7, "p", 4, BTreeSet::new(), now);
        let ev = |n: &str| Notice::Event { name: n.into() };
        let meta = |k: &str| Notice::Meta {
            key: k.into(),
            value: None,
        };
        // 진행 중 call 의 plugin 선언으로 판단한다.
        assert!(s.notice_allowed(o, 9, &ev("p-idle")));
        assert!(s.notice_allowed(o, 9, &ev(PROCESS_EXIT)));
        assert!(!s.notice_allowed(o, 9, &ev("bell")));
        assert!(!s.notice_allowed(o, 9, &meta("restore.command")));
        s.finish_call(o, 7, &BTreeSet::from([9]), Ok(serde_json::json!({})));
        assert!(s.notice_allowed(o, 9, &meta("p-note")));
        assert!(!s.notice_allowed(o, 9, &ev("q-idle")));
        assert!(!s.notice_allowed(o + 1, 9, &ev("p-idle")));
        let other = Notice::Plugin {
            namespace: "q".into(),
            payload: serde_json::json!({}),
        };
        assert!(!s.notice_allowed(o, 9, &other));
    }

    #[test]
    fn child_records_stay_while_notices_remain() {
        let now = Instant::now();
        let mut s = Server::new();
        let (_, o) = token(&s.hello(1, None, now));
        s.begin_call(1, 7, "p", 4, BTreeSet::new(), now);
        s.finish_call(o, 7, &BTreeSet::from([9]), Ok(serde_json::json!({})));
        s.relay(o, 4, 9, idle(), now);
        s.expire(now, |_| false);
        assert!(s.children.contains_key(&9));
        s.commit(1, 7, &[9]);
        let sent = s.due(now, |_| Some(1));
        let AgentServerMsg::ChildNotice { key, .. } = &sent[0].1 else {
            panic!()
        };
        s.ack(1, key, &AckOutcome::Recorded, now);
        s.expire(now, |_| false);
        assert!(!s.children.contains_key(&9));
    }
}
