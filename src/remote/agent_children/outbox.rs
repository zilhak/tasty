//! 원격 출처 자식의 통지를 client 확인(ACK) 전까지 보관하고 다시 보낸다.
//!
//! 보내기만 해서는 지우지 않는다. client 가 `recorded`·`duplicate`·`rejected` 로 답해야 지운다.
//! 답이 없으면 [`RESEND_AFTER`] 뒤에, 연결이 끊기면 다시 연결된 출처에 바로 같은 key 로 보낸다.
//! client 는 key 로 중복을 거른다. 서버 메모리에만 두므로 서버가 다시 시작하면 사라진다.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use tasty_ipc::agent_child::{AckOutcome, AgentServerMsg, Notice};

/// 보낸 뒤 확인을 기다리는 시간. 넘으면 같은 key 로 다시 보낸다.
pub(super) const RESEND_AFTER: Duration = Duration::from_secs(10);
/// client 가 나중에 처리하겠다고 답한 통지를 다시 보내기까지의 간격.
pub(super) const RETRY_LATER_AFTER: Duration = Duration::from_secs(1);
/// 확인받지 못한 통지를 보관하는 기간. 넘으면 버리고 경고를 남긴다.
pub(super) const TTL: Duration = Duration::from_secs(24 * 60 * 60);
/// 보관하는 통지 수 상한. 넘으면 가장 오래된 것부터 버린다.
pub(super) const CAP: usize = 1024;

pub(super) struct Entry {
    pub key: String,
    pub seq: u64,
    pub origin: u64,
    /// 통지를 만든 때 자식이 있던 서버 workspace. 자식이 사라진 뒤에도 holder 확인에 쓴다.
    pub workspace: u32,
    pub surface: u32,
    pub notice: Notice,
    created: Instant,
    next_at: Instant,
    sent_to: Option<u32>,
    attempts: u32,
}

impl Entry {
    fn message(&self) -> AgentServerMsg {
        AgentServerMsg::ChildNotice {
            key: self.key.clone(),
            seq: self.seq,
            remote_surface_id: self.surface,
            notice: self.notice.clone(),
        }
    }
}

pub(super) struct Outbox {
    prefix: String,
    seq: u64,
    entries: VecDeque<Entry>,
}

impl Outbox {
    /// `prefix` 는 서버 프로세스마다 다른 값이다. 재시작한 서버의 key 가 옛 key 와 겹치지 않는다.
    pub fn new(prefix: String) -> Self {
        Self {
            prefix,
            seq: 0,
            entries: VecDeque::new(),
        }
    }

    pub fn push(
        &mut self,
        origin: u64,
        workspace: u32,
        surface: u32,
        notice: Notice,
        now: Instant,
    ) -> String {
        self.seq += 1;
        let key = format!("{}.{}", self.prefix, self.seq);
        if self.entries.len() >= CAP
            && let Some(dropped) = self.entries.pop_front()
        {
            tracing::warn!(
                "agent child notice {} for s{} dropped unconfirmed: outbox full",
                dropped.key,
                dropped.surface
            );
        }
        self.entries.push_back(Entry {
            key: key.clone(),
            seq: self.seq,
            origin,
            workspace,
            surface,
            notice,
            created: now,
            next_at: now,
            sent_to: None,
            attempts: 0,
        });
        key
    }

    /// 보낼 차례인 통지를 만든 순서대로 고른다. `route` 는 그 출처를 지금 받을 수 있는 연결을 준다.
    /// 자식(surface)마다 확인받지 못한 가장 앞 통지 하나만 보낸다. 앞 통지가 대기 중이면 뒤 통지도
    /// 기다리므로 client 는 한 자식의 상태·이벤트·결과를 만든 순서대로 적용한다.
    pub fn due(
        &mut self,
        now: Instant,
        mut route: impl FnMut(&Entry) -> Option<u32>,
    ) -> Vec<(u32, AgentServerMsg)> {
        let mut out = Vec::new();
        let mut blocked = std::collections::HashSet::new();
        for entry in self.entries.iter_mut() {
            if !blocked.insert(entry.surface) {
                continue;
            }
            if entry.next_at > now {
                continue;
            }
            let Some(client) = route(entry) else {
                continue;
            };
            entry.sent_to = Some(client);
            entry.attempts = entry.attempts.saturating_add(1);
            entry.next_at = now + RESEND_AFTER;
            out.push((client, entry.message()));
        }
        out
    }

    pub fn holds_key(&self, key: &str) -> bool {
        self.entries.iter().any(|e| e.key == key)
    }

    /// 출처에 남은 통지가 있는 자식. 자식 기록을 지워도 되는지 판단한다.
    pub fn holds_surface(&self, surface: u32) -> bool {
        self.entries.iter().any(|e| e.surface == surface)
    }

    /// 출처가 확인한 통지를 정리한다. 다른 출처의 확인은 무시한다. 반영했으면 true.
    pub fn ack(&mut self, origin: u64, key: &str, outcome: &AckOutcome, now: Instant) -> bool {
        let Some(index) = self
            .entries
            .iter()
            .position(|e| e.key == key && e.origin == origin)
        else {
            return false;
        };
        if outcome.settles() {
            if let AckOutcome::Rejected { reason } = outcome {
                tracing::warn!(
                    "agent child notice {key} for s{} rejected by the client: {reason}",
                    self.entries[index].surface
                );
            }
            self.entries.remove(index);
        } else {
            let entry = &mut self.entries[index];
            entry.sent_to = None;
            entry.next_at = now + RETRY_LATER_AFTER;
        }
        true
    }

    /// 연결이 끊겼다. 그 연결로 보낸 통지는 결과를 모르므로 다음 연결에 바로 다시 보낸다.
    pub fn unsent_for(&mut self, client: u32, now: Instant) {
        for entry in self
            .entries
            .iter_mut()
            .filter(|e| e.sent_to == Some(client))
        {
            entry.sent_to = None;
            entry.next_at = now;
        }
    }

    /// 보관 기간이 지난 통지를 버린다. 버린 수를 돌려준다.
    pub fn expire(&mut self, now: Instant) -> usize {
        let before = self.entries.len();
        self.entries.retain(|e| {
            let keep = now.duration_since(e.created) < TTL;
            if !keep {
                tracing::warn!(
                    "agent child notice {} for s{} expired after {} attempts without confirmation",
                    e.key,
                    e.surface,
                    e.attempts
                );
            }
            keep
        });
        before - self.entries.len()
    }

    /// 조회용 요약. 본문은 싣지 않는다.
    pub fn summary(&self, now: Instant) -> Vec<serde_json::Value> {
        self.entries
            .iter()
            .map(|e| {
                serde_json::json!({
                    "key": e.key,
                    "origin": e.origin,
                    "surface_id": e.surface,
                    "workspace_id": e.workspace,
                    "kind": notice_kind(&e.notice),
                    "attempts": e.attempts,
                    "in_flight": e.sent_to.is_some(),
                    "age_ms": now.duration_since(e.created).as_millis() as u64,
                })
            })
            .collect()
    }
}

fn notice_kind(notice: &Notice) -> &'static str {
    match notice {
        Notice::State { .. } => "state",
        Notice::Event { .. } => "event",
        Notice::Meta { .. } => "meta",
        Notice::Plugin { .. } => "plugin",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn idle() -> Notice {
        Notice::Event {
            name: "claude-idle".into(),
        }
    }

    fn keys(sent: &[(u32, AgentServerMsg)]) -> Vec<(u32, String)> {
        sent.iter()
            .map(|(c, m)| match m {
                AgentServerMsg::ChildNotice { key, .. } => (*c, key.clone()),
                other => panic!("{other:?}"),
            })
            .collect()
    }

    #[test]
    fn a_sent_notice_stays_until_the_client_confirms_it() {
        let now = Instant::now();
        let mut out = Outbox::new("p".into());
        let key = out.push(1, 4, 9, idle(), now);
        assert_eq!(keys(&out.due(now, |_| Some(7))), vec![(7, key.clone())]);
        // 확인 전에는 같은 시각에 다시 보내지 않는다.
        assert!(out.due(now, |_| Some(7)).is_empty());
        // 확인이 오지 않으면 같은 key 로 다시 보낸다.
        let later = now + RESEND_AFTER;
        assert_eq!(keys(&out.due(later, |_| Some(7))), vec![(7, key.clone())]);
        assert!(out.ack(1, &key, &AckOutcome::Duplicate, later));
        assert!(out.due(later + RESEND_AFTER, |_| Some(7)).is_empty());
    }

    #[test]
    fn another_origin_cannot_confirm_or_receive_a_notice() {
        let now = Instant::now();
        let mut out = Outbox::new("p".into());
        let key = out.push(1, 4, 9, idle(), now);
        // 출처 1 을 받을 연결이 없으면 보내지 않는다(holder 교체·미연결).
        assert!(out.due(now, |e| (e.origin == 2).then_some(8)).is_empty());
        assert!(!out.ack(2, &key, &AckOutcome::Recorded, now));
        assert_eq!(out.summary(now).len(), 1);
    }

    #[test]
    fn a_disconnect_makes_in_flight_notices_due_again_for_the_next_connection() {
        let now = Instant::now();
        let mut out = Outbox::new("p".into());
        let key = out.push(1, 4, 9, idle(), now);
        out.due(now, |_| Some(7));
        out.unsent_for(7, now);
        assert_eq!(keys(&out.due(now, |_| Some(8))), vec![(8, key)]);
    }

    #[test]
    fn retry_later_waits_a_short_interval_and_rejected_settles() {
        let now = Instant::now();
        let mut out = Outbox::new("p".into());
        let a = out.push(1, 4, 9, idle(), now);
        let b = out.push(1, 4, 8, idle(), now);
        out.due(now, |_| Some(7));
        out.ack(1, &a, &AckOutcome::RetryLater, now);
        out.ack(1, &b, &AckOutcome::Rejected { reason: "x".into() }, now);
        assert!(out.due(now, |_| Some(7)).is_empty());
        assert_eq!(
            keys(&out.due(now + RETRY_LATER_AFTER, |_| Some(7))),
            vec![(7, a)]
        );
    }

    #[test]
    fn one_child_gets_its_notices_one_at_a_time_in_order() {
        let now = Instant::now();
        let mut out = Outbox::new("p".into());
        let a = out.push(1, 4, 9, idle(), now);
        let b = out.push(1, 4, 9, idle(), now);
        let other = out.push(1, 4, 8, idle(), now);
        assert_eq!(
            keys(&out.due(now, |_| Some(7))),
            vec![(7, a.clone()), (7, other)]
        );
        // a 가 나중에 처리되겠다고 답하면 b 도 a 뒤에서 기다린다.
        out.ack(1, &a, &AckOutcome::RetryLater, now);
        assert!(out.due(now, |_| Some(7)).is_empty());
        let later = now + RETRY_LATER_AFTER;
        assert_eq!(keys(&out.due(later, |_| Some(7))), vec![(7, a.clone())]);
        out.ack(1, &a, &AckOutcome::Recorded, later);
        assert_eq!(keys(&out.due(later, |_| Some(7))), vec![(7, b)]);
        assert!(out.holds_surface(9));
        assert_eq!(out.expire(now + TTL), 2);
    }

    #[test]
    fn keys_carry_the_process_prefix() {
        let mut out = Outbox::new("abc".into());
        assert_eq!(out.push(1, 1, 1, idle(), Instant::now()), "abc.1");
    }
}
