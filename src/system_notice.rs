//! 사용자 입력에서 시작하지 않은 작업(부팅 단계·자동 훅·웹훅·Lua 자동실행)의 결과를 알림 채널로 옮긴다.
//! 작업 스레드는 [`post`]로 남기고, GUI·headless 메인 루프가 [`take`]로 꺼내 시스템 출처 알림
//! intent로 바꾼다. 받을 workspace가 아직 없으면 꺼내지 않고 다음 회차를 기다린다.
//!
//! 같은 출처·레벨·제목·본문의 알림은 [`REPEAT_WINDOW`] 안에서 한 번만 남긴다. 대기 상한을
//! 넘으면 가장 오래된 항목을 버리고 경고 로그를 남긴다.

use std::collections::VecDeque;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::notification::NotificationLevel;

/// 같은 알림을 다시 남기지 않는 시간. 저장소의 짧은 합치기 간격보다 길게 잡아 반복 실패가 목록을 덮지 않게 한다.
pub(crate) const REPEAT_WINDOW: Duration = Duration::from_secs(10);
/// 메인 루프가 꺼내기 전에 쌓아 둘 수 있는 수.
const PENDING_LIMIT: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SystemNotice {
    pub level: NotificationLevel,
    pub title: String,
    pub body: String,
    /// 알림을 받을 surface. 0이거나 그 surface가 사라졌으면 활성 workspace가 받는다.
    pub surface_id: u32,
    /// GUI `notification.created` 이벤트의 source. 저장소에는 남지 않는다.
    pub source: &'static str,
}

impl SystemNotice {
    pub(crate) fn warning(source: &'static str, title: String, body: String) -> Self {
        Self::new(NotificationLevel::Warning, source, title, body)
    }

    pub(crate) fn new(
        level: NotificationLevel,
        source: &'static str,
        title: String,
        body: String,
    ) -> Self {
        Self {
            level,
            title,
            body,
            surface_id: 0,
            source,
        }
    }

    pub(crate) fn on_surface(mut self, surface_id: u32) -> Self {
        self.surface_id = surface_id;
        self
    }
}

type Waker = Arc<dyn Fn() + Send + Sync>;

#[derive(Default)]
struct Inbox {
    pending: VecDeque<SystemNotice>,
    recent: Vec<(SystemNotice, Instant)>,
}

impl Inbox {
    /// 새로 대기열에 넣었으면 true다. 반복 창 안의 같은 알림은 넣지 않는다.
    fn offer(&mut self, notice: SystemNotice, now: Instant) -> bool {
        self.recent
            .retain(|(_, at)| now.saturating_duration_since(*at) < REPEAT_WINDOW);
        if self.recent.iter().any(|(seen, _)| *seen == notice) {
            tracing::debug!(source = notice.source, "repeated system notice suppressed");
            return false;
        }
        self.recent.push((notice.clone(), now));
        if self.pending.len() >= PENDING_LIMIT
            && let Some(dropped) = self.pending.pop_front()
        {
            tracing::warn!(
                source = dropped.source,
                "system notice queue full; dropped the oldest: {}",
                dropped.title
            );
        }
        self.pending.push_back(notice);
        true
    }
}

struct Shared {
    inbox: Mutex<Inbox>,
    waker: OnceLock<Waker>,
}

static POISONED: AtomicBool = AtomicBool::new(false);

fn shared() -> &'static Shared {
    static SHARED: OnceLock<Shared> = OnceLock::new();
    SHARED.get_or_init(|| Shared {
        inbox: Mutex::new(Inbox::default()),
        waker: OnceLock::new(),
    })
}

fn lock() -> std::sync::MutexGuard<'static, Inbox> {
    crate::poison::recover_mutex(shared().inbox.lock(), "system notice inbox", &POISONED)
}

/// 메인 루프를 깨울 함수를 한 번 등록한다. 등록 전 알림은 다음 회차에 처리한다.
pub(crate) fn install_waker(waker: Waker) {
    if shared().waker.set(waker).is_err() {
        tracing::debug!("system notice waker already installed");
    }
}

/// 어느 스레드에서나 부른다. 경고·오류 로그는 호출자가 따로 남긴다.
pub(crate) fn post(notice: SystemNotice) {
    if lock().offer(notice, Instant::now())
        && let Some(wake) = shared().waker.get()
    {
        wake();
    }
}

/// 대기 중인 알림을 모두 꺼낸다. 받을 대상을 정한 뒤에 부른다.
pub(crate) fn take() -> Vec<SystemNotice> {
    lock().pending.drain(..).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notice(body: &str) -> SystemNotice {
        SystemNotice::warning("test", "title".into(), body.into())
    }

    #[test]
    fn a_repeat_inside_the_window_is_kept_once() {
        let mut inbox = Inbox::default();
        let t0 = Instant::now();
        assert!(inbox.offer(notice("a"), t0));
        assert!(!inbox.offer(notice("a"), t0 + Duration::from_secs(1)));
        assert!(inbox.offer(notice("b"), t0 + Duration::from_secs(1)));
        assert!(inbox.offer(notice("a"), t0 + REPEAT_WINDOW));
        let bodies: Vec<_> = inbox.pending.iter().map(|n| n.body.as_str()).collect();
        assert_eq!(bodies, ["a", "b", "a"]);
    }

    #[test]
    fn the_oldest_is_dropped_past_the_limit() {
        let mut inbox = Inbox::default();
        let t0 = Instant::now();
        for i in 0..=PENDING_LIMIT {
            inbox.offer(notice(&i.to_string()), t0);
        }
        assert_eq!(inbox.pending.len(), PENDING_LIMIT);
        assert_eq!(inbox.pending.front().map(|n| n.body.as_str()), Some("1"));
    }
}
