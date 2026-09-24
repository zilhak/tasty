//! 키별 타이머를 등록하고 실행할 시점이 된 키를 drain_due로 반환한다.
//! 콜백은 저장하지 않으며 호출자가 키에 해당하는 작업을 실행한다.
//! 시각은 호출자가 전달하므로 시험에서 기준시각을 직접 제어할 수 있다.
//! 통합 방법: docs/dev-guide/timer-hub.md.

// 이유: 테스트의 반환값 무시는 허용하되 제품 코드의 반환값 무시는 계속 검사한다.
#![cfg_attr(test, allow(clippy::let_underscore_must_use))]

mod waker;

use std::time::Duration;
use std::time::Instant;

pub use waker::TimerWakerHandle;
pub use waker::spawn_timer_waker;

/// 타이머가 깨우기를 요구하는 강도.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Precision {
    /// next_due를 깨우기 목표 시각으로 사용한다.
    Strict,
    /// next_due + slack을 깨우기 목표 시각으로 사용한다. 그보다 일찍 다른 이유로
    /// 깨어나도 next_due가 지났으면 drain_due에서 함께 반환한다.
    Lax { slack: Duration },
}

/// 등록된 타이머 1건의 관측용 스냅샷.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimerSnapshot<K> {
    pub key: K,
    /// 반복 주기. 일회성(`once_after`) 이면 `None`.
    pub interval: Option<Duration>,
    pub next_due: Instant,
    pub precision: Precision,
    /// 마지막으로 `drain_due` 가 이 키를 반환한 시각.
    pub last_fired: Option<Instant>,
}

#[derive(Debug, Clone, Copy)]
struct Entry<K> {
    key: K,
    /// `Some` = 반복, `None` = 일회성(발화 후 제거).
    interval: Option<Duration>,
    next_due: Instant,
    precision: Precision,
    last_fired: Option<Instant>,
}

impl<K> Entry<K> {
    /// 깨우기 목표 시각. Strict는 next_due, Lax는 next_due + slack이다.
    fn hard_deadline(&self) -> Instant {
        match self.precision {
            Precision::Strict => self.next_due,
            Precision::Lax { slack } => self.next_due + slack,
        }
    }
}

/// 키 기반 중앙 타이머 허브.
///
/// 등록 수는 수 개 규모(호스트의 주기 작업 전부를 합쳐도)라 선형 스캔으로 충분하다 —
/// 힙을 쓰면 `cancel`/재등록 시의 무효 항목 관리가 오히려 복잡해진다.
#[derive(Debug)]
pub struct TimerHub<K> {
    entries: Vec<Entry<K>>,
}

impl<K> Default for TimerHub<K> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
        }
    }
}

impl<K: Copy + Eq> TimerHub<K> {
    pub fn new() -> Self {
        Self::default()
    }

    /// `interval` 마다 반복 발화하는 타이머를 등록한다. 첫 발화는 `now + interval`.
    /// 같은 키가 이미 있으면 새 설정으로 교체한다(위상도 초기화된다).
    pub fn every(&mut self, key: K, interval: Duration, precision: Precision, now: Instant) {
        let interval = normalize(interval);
        self.insert(Entry {
            key,
            interval: Some(interval),
            next_due: now + interval,
            precision,
            last_fired: None,
        });
    }

    /// `delay` 후 한 번만 발화하는 타이머를 등록한다. 발화 시 자동 제거된다.
    /// 같은 키가 이미 있으면 새 설정으로 교체한다.
    pub fn once_after(&mut self, key: K, delay: Duration, precision: Precision, now: Instant) {
        self.insert(Entry {
            key,
            interval: None,
            next_due: now + delay,
            precision,
            last_fired: None,
        });
    }

    /// `at` 시각에 한 번만 발화하는 타이머를 등록한다. 상대 지연이 아니라 **절대
    /// 시각**이라, 매 프레임 같은 값으로 다시 불러도 위상이 밀리지 않는다 — 외부
    /// 상태(디바운스 시작 시각, backoff 다음 시도 시각 등)에서 파생한 데드라인을
    /// 선언적으로 동기화할 때 쓴다.
    pub fn once_at(&mut self, key: K, at: Instant, precision: Precision) {
        self.insert(Entry {
            key,
            interval: None,
            next_due: at,
            precision,
            last_fired: None,
        });
    }

    /// 등록을 해제한다. 없는 키면 no-op.
    pub fn cancel(&mut self, key: K) {
        self.entries.retain(|e| e.key != key);
    }

    /// 술어가 참인 키를 전부 해제한다. 파라미터화된 키(`Kind(id)`)를 살아있는 id
    /// 집합에 맞춰 정리할 때 쓴다 — 등록만 하고 해제를 잊으면 사라진 대상 때문에
    /// 영원히 깨어나는 누수가 된다.
    pub fn cancel_if(&mut self, mut pred: impl FnMut(K) -> bool) {
        self.entries.retain(|e| !pred(e.key));
    }

    pub fn is_registered(&self, key: K) -> bool {
        self.entries.iter().any(|e| e.key == key)
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// due 한 키만 등록 순서대로 반환하고 다음 발화 시각을 갱신한다.
    /// **실행은 호출자 몫이다** — 허브는 콜백을 소유하지 않는다.
    pub fn drain_due(&mut self, now: Instant) -> Vec<K> {
        let mut due = Vec::new();
        self.entries.retain_mut(|e| {
            if e.next_due > now {
                return true;
            }
            due.push(e.key);
            e.last_fired = Some(now);
            match e.interval {
                Some(interval) => {
                    e.next_due = advance(e.next_due, interval, now);
                    true
                }
                // 일회성 — 발화했으니 제거.
                None => false,
            }
        });
        due
    }

    /// 등록된 타이머의 가장 이른 hard deadline. Lax도 미래의 next_due + slack으로 즉시 참여한다.
    /// None은 등록된 타이머가 없다는 뜻이며 다른 이벤트의 깨우기 필요성은 판단하지 않는다.
    pub fn next_deadline(&self) -> Option<Instant> {
        self.entries.iter().map(Entry::hard_deadline).min()
    }

    /// 관측용 스냅샷(등록 순서).
    pub fn snapshot(&self) -> Vec<TimerSnapshot<K>> {
        self.entries
            .iter()
            .map(|e| TimerSnapshot {
                key: e.key,
                interval: e.interval,
                next_due: e.next_due,
                precision: e.precision,
                last_fired: e.last_fired,
            })
            .collect()
    }

    fn insert(&mut self, entry: Entry<K>) {
        match self.entries.iter_mut().find(|e| e.key == entry.key) {
            Some(slot) => *slot = entry,
            None => self.entries.push(entry),
        }
    }
}

/// 0 주기를 최소 단위로 바꿔 다음 due가 같은 시각에 머무르지 않게 한다.
fn normalize(interval: Duration) -> Duration {
    if interval.is_zero() {
        Duration::from_nanos(1)
    } else {
        interval
    }
}

/// 반복 타이머의 다음 발화 시각. **누적 드리프트가 없도록** 직전 데드라인에
/// `interval` 배수를 더해 위상을 유지한다(`now` 기준으로 다시 재지 않는다).
///
/// 절전 복귀처럼 크게 밀린 경우엔 배수가 폭증하므로 `now` 기준으로 재정렬한다 —
/// 밀린 만큼을 몰아서 발화시키지 않는 것이 의도다(허브는 발화를 큐잉하지 않는다).
fn advance(next_due: Instant, interval: Duration, now: Instant) -> Instant {
    let behind = now.saturating_duration_since(next_due);
    let steps = behind.as_nanos() / interval.as_nanos() + 1;
    match u32::try_from(steps)
        .ok()
        .and_then(|s| interval.checked_mul(s))
    {
        Some(delta) => next_due + delta,
        None => now + interval,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum K {
        Busy,
        Sweep,
        Menu,
    }

    const S30: Duration = Duration::from_secs(30);
    const S60: Duration = Duration::from_secs(60);

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    #[test]
    fn empty_hub_has_no_deadline() {
        assert!(TimerHub::<K>::new().next_deadline().is_none());
    }

    #[test]
    fn strict_timer_sets_the_hard_deadline() {
        let t0 = Instant::now();
        let mut hub = TimerHub::new();
        hub.every(K::Busy, secs(1), Precision::Strict, t0);
        assert_eq!(hub.next_deadline(), Some(t0 + secs(1)));
    }

    #[test]
    fn lax_timer_does_not_advance_the_deadline_before_slack() {
        let t0 = Instant::now();
        let mut hub = TimerHub::new();
        hub.every(K::Sweep, S30, Precision::Lax { slack: S60 }, t0);
        // 30초 데드라인이지만 hard deadline 은 slack 을 더한 90초다.
        assert_eq!(hub.next_deadline(), Some(t0 + secs(90)));
        // 그럼에도 다른 이유로 깨어난 프레임에서는 함께 실행된다.
        assert_eq!(hub.drain_due(t0 + secs(31)), vec![K::Sweep]);
    }

    #[test]
    fn lax_timer_is_promoted_to_hard_after_slack() {
        let t0 = Instant::now();
        let mut hub = TimerHub::new();
        hub.every(K::Sweep, S30, Precision::Lax { slack: S60 }, t0);
        // deadline(30s) + slack(60s) = 90s 에는 반드시 깨워야 한다.
        assert_eq!(hub.next_deadline(), Some(t0 + secs(90)));
        // 89초엔 아직 due 지만(30초 경과) hard deadline 은 그대로.
        assert_eq!(hub.drain_due(t0 + secs(89)), vec![K::Sweep]);
    }

    #[test]
    fn strict_timer_wins_the_deadline_over_lax() {
        let t0 = Instant::now();
        let mut hub = TimerHub::new();
        hub.every(K::Sweep, S30, Precision::Lax { slack: S60 }, t0);
        hub.every(K::Busy, secs(1), Precision::Strict, t0);
        assert_eq!(hub.next_deadline(), Some(t0 + secs(1)));
    }

    #[test]
    fn not_yet_due_returns_nothing() {
        let t0 = Instant::now();
        let mut hub = TimerHub::new();
        hub.every(K::Busy, secs(1), Precision::Strict, t0);
        assert!(hub.drain_due(t0 + Duration::from_millis(999)).is_empty());
        assert_eq!(hub.drain_due(t0 + secs(1)), vec![K::Busy]);
    }

    #[test]
    fn repeating_timer_does_not_accumulate_drift() {
        let t0 = Instant::now();
        let mut hub = TimerHub::new();
        hub.every(K::Busy, secs(1), Precision::Strict, t0);
        // 1s 타이머를 2.5s 시점에 drain 하면 다음 데드라인은 3.0s (2.0s 가 아님).
        assert_eq!(
            hub.drain_due(t0 + Duration::from_millis(2500)),
            vec![K::Busy]
        );
        assert_eq!(hub.next_deadline(), Some(t0 + secs(3)));
        // 그 뒤로도 위상이 t0 기준으로 유지된다.
        assert_eq!(hub.drain_due(t0 + secs(3)), vec![K::Busy]);
        assert_eq!(hub.next_deadline(), Some(t0 + secs(4)));
    }

    #[test]
    fn huge_lag_realigns_to_now_instead_of_replaying() {
        let t0 = Instant::now();
        let mut hub = TimerHub::new();
        hub.every(K::Busy, Duration::from_nanos(1), Precision::Strict, t0);
        // u32 배수로 표현할 수 없을 만큼 밀린 경우 — now 기준 재정렬.
        let late = t0 + secs(3600);
        assert_eq!(hub.drain_due(late), vec![K::Busy]);
        assert_eq!(hub.next_deadline(), Some(late + Duration::from_nanos(1)));
    }

    #[test]
    fn once_after_fires_exactly_once() {
        let t0 = Instant::now();
        let mut hub = TimerHub::new();
        hub.once_after(K::Menu, Duration::from_millis(8), Precision::Strict, t0);
        assert!(hub.is_registered(K::Menu));
        assert_eq!(hub.next_deadline(), Some(t0 + Duration::from_millis(8)));
        assert_eq!(hub.drain_due(t0 + Duration::from_millis(8)), vec![K::Menu]);
        assert!(!hub.is_registered(K::Menu));
        assert!(hub.next_deadline().is_none());
        assert!(hub.drain_due(t0 + secs(1)).is_empty());
    }

    #[test]
    fn re_registering_a_key_replaces_it() {
        let t0 = Instant::now();
        let mut hub = TimerHub::new();
        hub.once_after(K::Menu, Duration::from_millis(8), Precision::Strict, t0);
        // 같은 키를 다시 등록하면 항목이 늘지 않고 위상만 밀린다.
        hub.once_after(
            K::Menu,
            Duration::from_millis(8),
            Precision::Strict,
            t0 + Duration::from_millis(5),
        );
        assert_eq!(hub.snapshot().len(), 1);
        assert_eq!(hub.next_deadline(), Some(t0 + Duration::from_millis(13)));
    }

    #[test]
    fn once_at_registers_an_absolute_deadline() {
        let t0 = Instant::now();
        let mut hub = TimerHub::new();
        hub.once_at(K::Sweep, t0 + secs(5), Precision::Strict);
        assert_eq!(hub.next_deadline(), Some(t0 + secs(5)));
        // 같은 절대 시각으로 다시 등록해도 위상이 밀리지 않는다(선언적 동기화).
        hub.once_at(K::Sweep, t0 + secs(5), Precision::Strict);
        assert_eq!(hub.snapshot().len(), 1);
        assert_eq!(hub.next_deadline(), Some(t0 + secs(5)));
        assert_eq!(hub.drain_due(t0 + secs(5)), vec![K::Sweep]);
        assert!(!hub.is_registered(K::Sweep));
    }

    #[test]
    fn cancel_if_removes_every_matching_key() {
        let t0 = Instant::now();
        let mut hub = TimerHub::new();
        hub.every(K::Busy, secs(1), Precision::Strict, t0);
        hub.every(K::Sweep, secs(3), Precision::Strict, t0);
        hub.every(K::Menu, secs(3), Precision::Strict, t0);
        hub.cancel_if(|k| matches!(k, K::Sweep | K::Menu));
        assert!(hub.is_registered(K::Busy));
        assert!(!hub.is_registered(K::Sweep));
        assert!(!hub.is_registered(K::Menu));
    }

    #[test]
    fn cancel_removes_the_timer() {
        let t0 = Instant::now();
        let mut hub = TimerHub::new();
        hub.every(K::Busy, secs(1), Precision::Strict, t0);
        hub.cancel(K::Busy);
        assert!(!hub.is_registered(K::Busy));
        assert!(hub.is_empty());
        assert!(hub.next_deadline().is_none());
        // 없는 키 취소는 no-op.
        hub.cancel(K::Sweep);
    }

    #[test]
    fn drain_returns_every_due_key() {
        let t0 = Instant::now();
        let mut hub = TimerHub::new();
        hub.every(K::Busy, secs(1), Precision::Strict, t0);
        hub.every(K::Sweep, secs(3), Precision::Strict, t0);
        assert_eq!(hub.drain_due(t0 + secs(1)), vec![K::Busy]);
        assert_eq!(hub.drain_due(t0 + secs(3)), vec![K::Busy, K::Sweep]);
    }

    #[test]
    fn snapshot_reports_registration_state() {
        let t0 = Instant::now();
        let mut hub = TimerHub::new();
        hub.every(K::Busy, secs(1), Precision::Strict, t0);
        let before = hub.snapshot();
        assert_eq!(before.len(), 1);
        assert_eq!(before[0].key, K::Busy);
        assert_eq!(before[0].interval, Some(secs(1)));
        assert_eq!(before[0].next_due, t0 + secs(1));
        assert_eq!(before[0].precision, Precision::Strict);
        assert!(before[0].last_fired.is_none());

        // 반환 키는 여기서 의미 없다 — last_fired 갱신만 확인한다.
        let _ = hub.drain_due(t0 + secs(1));
        assert_eq!(hub.snapshot()[0].last_fired, Some(t0 + secs(1)));
    }

    #[test]
    fn zero_interval_is_normalized_to_a_nonzero_tick() {
        let t0 = Instant::now();
        let mut hub = TimerHub::new();
        hub.every(K::Busy, Duration::ZERO, Precision::Strict, t0);
        // 0 주기는 최소 단위로 바뀌어야 한다.
        assert!(hub.next_deadline().is_some_and(|d| d > t0));
    }
}
