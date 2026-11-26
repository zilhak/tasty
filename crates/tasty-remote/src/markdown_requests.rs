//! 보낸 markdown 원문 요청의 회신 마감. 시각은 호출자가 넘기며 App·View를 참조하지 않는다.
//!
//! 연결이 살아 있어도 원격이 회신하지 않으면 요청을 기다리는 쪽이 끝나지 않는다. 마감이 지난
//! 요청은 꺼내 실패로 끝내고, 꺼낸 뒤 온 회신은 기다리던 요청이 아니므로 버린다.
//! 규칙: docs/dev-guide/attach-behavior.md#markdown-content-채널.

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// 원문 요청을 보낸 뒤 회신을 기다리는 시간. 지나면 실패로 끝낸다.
pub const MARKDOWN_CONTENT_REPLY_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Pending {
    local_surface_id: u32,
    deadline: Instant,
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
            },
        );
    }

    /// 회신을 받았다. 기다리던 요청이면 지우고 true, 만료 처리(`take_due`)로 이미 꺼냈거나 모르는
    /// 요청이면 false. 시각은 보지 않으므로 마감이 지났어도 아직 꺼내지 않은 요청이면 true다.
    pub fn answer(&mut self, request_id: u64) -> bool {
        self.pending.remove(&request_id).is_some()
    }

    /// 마감이 지난 요청을 (request_id, 로컬 surface) 로 꺼낸다. 마감 시각 자체도 지난 것으로 본다.
    pub fn take_due(&mut self, now: Instant) -> Vec<(u64, u32)> {
        let mut due: Vec<(u64, u32)> = self
            .pending
            .iter()
            .filter(|(_, pending)| pending.deadline <= now)
            .map(|(&id, pending)| (id, pending.local_surface_id))
            .collect();
        due.sort_unstable();
        for (id, _) in &due {
            self.pending.remove(id);
        }
        due
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

    #[test]
    fn a_request_is_due_exactly_at_its_deadline() {
        let mut requests = MarkdownRequests::default();
        let t0 = Instant::now();
        requests.note(4, 300, t0);
        let deadline = t0 + MARKDOWN_CONTENT_REPLY_TIMEOUT;
        assert!(
            requests
                .take_due(deadline - Duration::from_millis(1))
                .is_empty()
        );
        assert_eq!(requests.take_due(deadline), vec![(4, 300)]);
        assert!(
            requests
                .take_due(deadline + Duration::from_secs(1))
                .is_empty(),
            "a request is failed once"
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
            requests.take_due(t0 + MARKDOWN_CONTENT_REPLY_TIMEOUT),
            vec![(5, 301)],
            "only the unanswered request falls due"
        );
    }

    #[test]
    fn a_reply_after_the_request_expired_is_not_an_answer() {
        let mut requests = MarkdownRequests::default();
        let t0 = Instant::now();
        requests.note(4, 300, t0);
        assert_eq!(
            requests.take_due(t0 + MARKDOWN_CONTENT_REPLY_TIMEOUT),
            vec![(4, 300)]
        );
        assert!(!requests.answer(4), "the late reply is dropped");
        assert!(!requests.answer(9), "an unknown request is not an answer");
    }

    #[test]
    fn taking_all_requests_ends_every_wait() {
        let mut requests = MarkdownRequests::default();
        let t0 = Instant::now();
        requests.note(5, 301, t0);
        requests.note(4, 300, t0);
        assert_eq!(requests.take_all(), vec![(4, 300), (5, 301)]);
        assert!(!requests.answer(4));
        assert!(
            requests
                .take_due(t0 + MARKDOWN_CONTENT_REPLY_TIMEOUT)
                .is_empty()
        );
    }

    #[test]
    fn forgotten_requests_neither_fall_due_nor_answer() {
        let mut requests = MarkdownRequests::default();
        let t0 = Instant::now();
        requests.note(4, 300, t0);
        requests.forget_all();
        assert!(
            requests
                .take_due(t0 + MARKDOWN_CONTENT_REPLY_TIMEOUT)
                .is_empty()
        );
        assert!(!requests.answer(4));
    }
}
