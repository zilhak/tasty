//! Intent / 명령 큐를 *단일 자료구조* 로 통합하기 위한 최소 도구.
//!
//! 설계 배경: `.claude-workspace/plans/phase-d/intent-ui-vs-domain.md`.
//!
//! - **단일 큐**. priority / lazy / hook 같은 분리 없음 (over-engineered 로 폐기).
//! - **Envelope 는 minimal** — `body` / `origin` / `trace_id` 만. expiry / deadline
//!   / priority 같은 필드 없음.
//! - **별 crate 아님** — 본 파일 한 장.
//!
//! 사용처: D.3.I.3 이후 `state.pending_intents` + `state.pending_core_intents`
//! 두 필드가 단일 `Scheduler<Intent>` 로 통합되며 이 모듈을 사용한다. 현재는
//! 도구만 정의돼 있고 호출처가 0 이다.

use std::collections::VecDeque;

/// 큐를 통과하는 모든 명령은 `Envelope<B>` 로 감싸진다. *origin* (발화 주체) 과
/// *trace_id* (cascade chain 추적) 만 본문에 동봉.
#[derive(Debug, Clone)]
#[allow(dead_code)] // D.3.I.3 까지 사용처 없음.
pub(crate) struct Envelope<B> {
    pub body: B,
    pub origin: Origin,
    /// `None` 이면 dispatch 시점에 새 UUID 발급, `Some` 이면 그대로 전파.
    pub trace_id: Option<String>,
}

/// 명령을 발화한 주체. 기존 `crate::intent::IntentOrigin` 과 동등한 표현이지만
/// Scheduler 가 `crate::intent` 에 의존하지 않도록 본 모듈에 별도로 둔다 —
/// D.3.I.3 에서 `IntentOrigin` 을 본 타입에 흡수할 예정.
///
/// release 표면의 발화 주체는 `User` (단축키/메뉴/컨텍스트메뉴) 또는 `System`
/// (cascade / 도메인 boot / Adapter 의 자동 작업). `Agent` 는 *release IPC/CLI*
/// 의 도메인 발화 (popup 발화 불가 — `docs/design/popup-system.md` "Popup 발화
/// 정책 (CRITICAL)") 와 *debug 빌드의 사용자 입력 재현* 양쪽을 의미한다.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) enum Origin {
    User(UserSource),
    Agent(AgentSource),
    System,
    /// 이전 발화에서 cascade. 부모의 origin 을 그대로 보존하므로 별도 variant
    /// 가 아니라 *별 메타데이터*. (envelope 생성 시 `cascaded_from` helper 사용)
    Cascade {
        parent_trace_id: String,
    },
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) enum UserSource {
    Shortcut(&'static str),
    Menu(&'static str),
    ContextMenu,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) enum AgentSource {
    Ipc,
    Plugin(String),
    Cli,
}

/// 명령 큐 자체. 단순 FIFO. drain 후 모두 처리 — 한 frame 안의 cascade 도 같은
/// drain 라운드 안에서 모두 소비된다.
///
/// 동시에 여러 곳에서 enqueue 가능하지만 본 구조는 `&mut self` 단일 owner 전제.
/// 멀티-thread 발화는 *호출자가 channel 로 main loop 에 전달* 후 main loop 가
/// `enqueue` 한다 — Scheduler 가 직접 thread-safe 가 되지는 않음.
#[derive(Debug, Default)]
#[allow(dead_code)]
pub(crate) struct Scheduler<B> {
    queue: VecDeque<Envelope<B>>,
}

#[allow(dead_code)]
impl<B> Scheduler<B> {
    pub fn new() -> Self {
        Self {
            queue: VecDeque::new(),
        }
    }

    pub fn enqueue(&mut self, env: Envelope<B>) {
        self.queue.push_back(env);
    }

    /// 큐를 모두 비우고 발화 순서대로 반환. drain 중 새로 enqueue 된 항목은
    /// 다음 drain 라운드에 처리된다 (재진입 방지).
    pub fn drain(&mut self) -> Vec<Envelope<B>> {
        std::mem::take(&mut self.queue).into_iter().collect()
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    pub fn len(&self) -> usize {
        self.queue.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enqueue_then_drain_preserves_order() {
        let mut s: Scheduler<u32> = Scheduler::new();
        s.enqueue(Envelope {
            body: 1,
            origin: Origin::System,
            trace_id: None,
        });
        s.enqueue(Envelope {
            body: 2,
            origin: Origin::System,
            trace_id: None,
        });
        let drained = s.drain();
        assert_eq!(drained.len(), 2);
        assert_eq!(drained[0].body, 1);
        assert_eq!(drained[1].body, 2);
        assert!(s.is_empty());
    }

    #[test]
    fn drain_during_processing_does_not_see_new_pushes() {
        // 한 frame drain 안에서 새 enqueue 가 *현재 batch* 에 포함되지 않음을
        // 확인 — 본 보장은 dispatcher loop 의 재진입 방지와 일치한다.
        let mut s: Scheduler<u32> = Scheduler::new();
        s.enqueue(Envelope {
            body: 1,
            origin: Origin::System,
            trace_id: None,
        });
        let batch = s.drain();
        // batch 를 처리하는 동안 새 enqueue.
        s.enqueue(Envelope {
            body: 2,
            origin: Origin::System,
            trace_id: None,
        });
        assert_eq!(batch.len(), 1);
        assert_eq!(s.len(), 1);
    }
}
