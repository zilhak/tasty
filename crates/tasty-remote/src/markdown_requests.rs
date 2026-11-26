//! 보낸 markdown 원문 요청의 회신 마감. 시각은 호출자가 넘기며 App·View를 참조하지 않는다.
//!
//! 연결이 살아 있어도 원격이 회신하지 않으면 요청을 기다리는 쪽이 끝나지 않는다. 마감이 지난
//! 요청은 만료로 표시해 실패로 끝내고, 만료 뒤 온 회신은 기다리던 요청이 아니므로 버린다. 만료한
//! 요청은 실패를 plugin에 넘길 때까지 남아 다음 회차에 다시 넘긴다.
//! 규칙: docs/dev-guide/attach-behavior.md#markdown-content-채널.

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// 원문 요청을 보낸 뒤 회신을 기다리는 시간. 지나면 실패로 끝낸다.
pub const MARKDOWN_CONTENT_REPLY_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Pending {
    local_surface_id: u32,
    deadline: Instant,
    /// 마감이 지나 실패로 끝낼 요청이다. 실패를 넘기면(`settle`) 지운다.
    expired: bool,
}

/// 실패로 끝내야 하는 만료 요청.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DueRequest {
    pub request_id: u64,
    pub local_surface_id: u32,
    /// 이번 `due` 호출에서 마감이 지났다. false면 앞 회차에 만료했지만 실패를 아직 넘기지 못했다.
    pub newly_expired: bool,
}

/// 회신을 기다리는 원문 요청(request_id → 로컬 mirror surface와 마감).
#[derive(Debug, Default)]
pub struct MarkdownRequests {
    pending: HashMap<u64, Pending>,
}

impl MarkdownRequests {
    /// 보낸 요청을 기다리기 시작한다.
    pub fn note(&mut self, request_id: u64, local_surface_id: u32, sent_at: Instant) {
        self.pending.insert(
            request_id,
            Pending {
                local_surface_id,
                deadline: sent_at + MARKDOWN_CONTENT_REPLY_TIMEOUT,
                expired: false,
            },
        );
    }

    /// 회신을 받았다. 기다리던 요청이면 지우고 true, 만료한 요청이거나 모르는 요청이면 false다.
    /// 만료한 요청은 실패를 넘길 때까지 남긴다. 시각은 보지 않으므로 마감이 지났어도 아직 `due`가
    /// 만료로 표시하지 않은 요청이면 true다.
    pub fn answer(&mut self, request_id: u64) -> bool {
        match self.pending.get(&request_id) {
            Some(pending) if !pending.expired => {
                self.pending.remove(&request_id);
                true
            }
            _ => false,
        }
    }

    /// 마감이 지난 요청을 만료로 표시하고, 실패를 아직 넘기지 않은 만료 요청을 request_id 순으로
    /// 돌려준다. 마감 시각 자체도 지난 것으로 본다. 실패를 넘긴 요청은 `settle`로 지운다.
    pub fn due(&mut self, now: Instant) -> Vec<DueRequest> {
        let mut due: Vec<DueRequest> = Vec::new();
        for (&request_id, pending) in &mut self.pending {
            let newly_expired = !pending.expired && pending.deadline <= now;
            pending.expired |= newly_expired;
            if pending.expired {
                due.push(DueRequest {
                    request_id,
                    local_surface_id: pending.local_surface_id,
                    newly_expired,
                });
            }
        }
        due.sort_unstable_by_key(|request| request.request_id);
        due
    }

    /// 만료 요청의 실패를 넘겼다. 그 요청을 지운다.
    pub fn settle(&mut self, request_id: u64) {
        self.pending.remove(&request_id);
    }

    /// 연결이 바뀌어 회신이 오지 않을 요청을 모두 (request_id, 로컬 surface) 로 꺼낸다.
    pub fn take_all(&mut self) -> Vec<(u64, u32)> {
        let mut all: Vec<(u64, u32)> = self
            .pending
            .drain()
            .map(|(id, pending)| (id, pending.local_surface_id))
            .collect();
        all.sort_unstable();
        all
    }

    /// 연결이 바뀌어 회신이 오지 않을 요청을 모두 버린다.
    pub fn forget_all(&mut self) {
        self.pending.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn due(request_id: u64, local_surface_id: u32, newly_expired: bool) -> DueRequest {
        DueRequest {
            request_id,
            local_surface_id,
            newly_expired,
        }
    }

    #[test]
    fn a_request_is_due_exactly_at_its_deadline() {
        let mut requests = MarkdownRequests::default();
        let t0 = Instant::now();
        requests.note(4, 300, t0);
        let deadline = t0 + MARKDOWN_CONTENT_REPLY_TIMEOUT;
        assert!(requests.due(deadline - Duration::from_millis(1)).is_empty());
        assert_eq!(requests.due(deadline), vec![due(4, 300, true)]);
        requests.settle(4);
        assert!(
            requests.due(deadline + Duration::from_secs(1)).is_empty(),
            "a request is failed once"
        );
    }

    #[test]
    fn an_expired_request_stays_due_until_its_failure_is_settled() {
        let mut requests = MarkdownRequests::default();
        let t0 = Instant::now();
        requests.note(4, 300, t0);
        let deadline = t0 + MARKDOWN_CONTENT_REPLY_TIMEOUT;
        assert_eq!(requests.due(deadline), vec![due(4, 300, true)]);
        assert_eq!(
            requests.due(deadline + Duration::from_secs(1)),
            vec![due(4, 300, false)],
            "the failure that did not reach the plugin is owed again, as an old expiry"
        );
        assert!(
            !requests.answer(4),
            "a late reply while the failure is owed is not an answer"
        );
        assert_eq!(
            requests.due(deadline + Duration::from_secs(2)),
            vec![due(4, 300, false)],
            "the late reply does not cancel the owed failure"
        );
    }

    #[test]
    fn an_answered_request_never_falls_due() {
        let mut requests = MarkdownRequests::default();
        let t0 = Instant::now();
        requests.note(4, 300, t0);
        requests.note(5, 301, t0);
        assert!(requests.answer(4));
        assert_eq!(
            requests.due(t0 + MARKDOWN_CONTENT_REPLY_TIMEOUT),
            vec![due(5, 301, true)],
            "only the unanswered request falls due"
        );
    }

    #[test]
    fn a_reply_after_the_request_expired_is_not_an_answer() {
        let mut requests = MarkdownRequests::default();
        let t0 = Instant::now();
        requests.note(4, 300, t0);
        assert_eq!(
            requests.due(t0 + MARKDOWN_CONTENT_REPLY_TIMEOUT),
            vec![due(4, 300, true)]
        );
        requests.settle(4);
        assert!(!requests.answer(4), "the late reply is dropped");
        assert!(!requests.answer(9), "an unknown request is not an answer");
    }

    #[test]
    fn taking_all_requests_ends_every_wait() {
        let mut requests = MarkdownRequests::default();
        let t0 = Instant::now();
        requests.note(5, 301, t0);
        requests.note(4, 300, t0);
        assert_eq!(
            requests.due(t0 + MARKDOWN_CONTENT_REPLY_TIMEOUT / 2),
            Vec::new()
        );
        let overdue = t0
            .checked_sub(MARKDOWN_CONTENT_REPLY_TIMEOUT)
            .expect("the clock reaches back one deadline");
        requests.note(6, 302, overdue);
        assert_eq!(requests.due(t0), vec![due(6, 302, true)]);
        assert_eq!(
            requests.take_all(),
            vec![(4, 300), (5, 301), (6, 302)],
            "an expired request whose failure is still owed ends with the rest"
        );
        assert!(!requests.answer(4));
        assert!(requests.due(t0 + MARKDOWN_CONTENT_REPLY_TIMEOUT).is_empty());
    }

    #[test]
    fn forgotten_requests_neither_fall_due_nor_answer() {
        let mut requests = MarkdownRequests::default();
        let t0 = Instant::now();
        requests.note(4, 300, t0);
        requests.forget_all();
        assert!(requests.due(t0 + MARKDOWN_CONTENT_REPLY_TIMEOUT).is_empty());
        assert!(!requests.answer(4));
    }
}
