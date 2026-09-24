//! 웹훅 매칭·인증 실패를 IP별로 세고 일정 시간 차단한다.
//! 404·405·401·413을 세며 200·410·429는 세지 않는다. 같은 IP의 실패가
//! window 안에서 threshold에 도달하면 cooldown 동안 매칭 전에 429로 거부한다.
//! 이때 같은 IP의 정상 요청도 차단되며 재시작하면 카운터와 차단 상태가 사라진다.
//!
//! 시간은 인자로 받아 시험에서 통제한다. 전역 진입점은 Instant::now()를 사용한다.
//! [Webhook 접수 정책](../../docs/adr/0032-webhook-admission.md)과
//! [락 poison 처리](../../docs/dev-guide/error-handling.md#락-poison-mutex--rwlock)를 따른다.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use super::ack::AckStatus;

/// 출처 표의 크기가 이 값을 넘어야 정리를 시도한다. 메모리 상한은 아니다.
///
/// 쿨다운 중이거나 윈도우 안인 항목은 남기므로 표가 문턱을 넘어 계속 커질 수 있다.
/// 실제 크기는 유입량뿐 아니라 항목별 윈도우·쿨다운과 정리 시점에도 영향을 받는다.
/// 유입이 멎어도 시간 경과만으로 정리하지 않는다. 이후 실패 기록이 prune에 도달했을 때도
/// 크기와 순회 간격 조건을 통과해야 만료 항목을 지운다.
/// 정책은 [Webhook 접수 결정](../../docs/adr/0032-webhook-admission.md)을 따른다.
const PRUNE_TRIGGER_SOURCES: usize = 4096;

/// 전체 표를 훑는 최소 간격. 크기 문턱을 넘은 뒤에도 이 간격 안에는 다시 순회하지 않는다.
///
/// 실패마다 순회하면 전역 락을 잡고 표 전체를 반복 읽으므로 간격을 윈도우 길이로 둔다.
/// 그 사이 새로 만료된 항목이 생길 수 있지만 다음 허용된 순회까지 정리를 미룬다.
/// 이후 실패가 없거나 표 크기가 문턱 이하라면 그보다 오래 남을 수도 있다.
fn prune_interval(cfg: AbuseConfig) -> Duration {
    cfg.window
}

#[derive(Debug, Clone, Copy)]
pub struct AbuseConfig {
    /// `window` 안에서 이 횟수 이상 실패하면 쿨다운으로 전환.
    pub threshold: u32,
    /// 실패를 셀 시간 범위. 이후 실패를 기록할 때 지난 범위의 카운터를 초기화한다.
    pub window: Duration,
    /// 임계치 도달 후 429 거부를 유지하는 시간.
    pub cooldown: Duration,
}

/// 기본값의 근거는 [ADR-0032](../../docs/adr/0032-webhook-admission.md)를 참고한다.
impl Default for AbuseConfig {
    fn default() -> Self {
        Self {
            threshold: 20,
            window: Duration::from_secs(10),
            cooldown: Duration::from_secs(60),
        }
    }
}

fn env_u64(key: &str) -> Option<u64> {
    std::env::var(key)
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
}

impl AbuseConfig {
    /// env 오버라이드 적용(없거나 0/파싱실패면 기본값 유지).
    ///
    /// - `TASTY_WEBHOOK_ABUSE_THRESHOLD` — 임계치(회)
    /// - `TASTY_WEBHOOK_ABUSE_WINDOW_SECS` — 윈도우(초)
    /// - `TASTY_WEBHOOK_ABUSE_COOLDOWN_SECS` — 쿨다운(초)
    pub fn from_env() -> Self {
        let d = Self::default();
        Self {
            threshold: env_u64("TASTY_WEBHOOK_ABUSE_THRESHOLD")
                .filter(|v| *v > 0)
                .map(|v| v.min(u32::MAX as u64) as u32)
                .unwrap_or(d.threshold),
            window: env_u64("TASTY_WEBHOOK_ABUSE_WINDOW_SECS")
                .filter(|v| *v > 0)
                .map(Duration::from_secs)
                .unwrap_or(d.window),
            cooldown: env_u64("TASTY_WEBHOOK_ABUSE_COOLDOWN_SECS")
                .filter(|v| *v > 0)
                .map(Duration::from_secs)
                .unwrap_or(d.cooldown),
        }
    }
}

#[derive(Debug)]
struct SourceState {
    /// 현재 카운팅 윈도우 시작 시각.
    window_start: Instant,
    /// 현재 윈도우에서 누적된 실패 횟수.
    fail_count: u32,
    /// 쿨다운 종료 시각(설정돼 있으면 그 전까지 즉시 거부).
    cooldown_until: Option<Instant>,
}

#[derive(Debug)]
pub struct AbuseTracker {
    config: AbuseConfig,
    sources: HashMap<String, SourceState>,
    /// 마지막 실제 정리 시각. 생성 시에는 시계를 읽지 않는다.
    last_prune: Option<Instant>,
}

impl AbuseTracker {
    pub fn new(config: AbuseConfig) -> Self {
        Self {
            config,
            sources: HashMap::new(),
            last_prune: None,
        }
    }

    /// 이 출처가 쿨다운 중인가(참이면 즉시 429). 만료된 쿨다운은 여기서 해제한다.
    pub fn is_blocked(&mut self, source: &str, now: Instant) -> bool {
        if let Some(st) = self.sources.get_mut(source)
            && let Some(until) = st.cooldown_until
        {
            if now < until {
                return true;
            }
            st.cooldown_until = None;
            st.fail_count = 0;
            st.window_start = now;
        }
        false
    }

    /// 출처 실패 1회 기록([`counts_as_failure`] 가 고른 것만 온다). 윈도우 내
    /// 누적이 임계치에 도달하면 쿨다운 시작.
    pub fn record_failure(&mut self, source: &str, now: Instant) {
        let cfg = self.config;
        let st = self
            .sources
            .entry(source.to_string())
            .or_insert_with(|| SourceState {
                window_start: now,
                fail_count: 0,
                cooldown_until: None,
            });
        // 이미 차단 중이면 실패로 종료 시각을 연장하지 않는다.
        if let Some(until) = st.cooldown_until {
            if now < until {
                return;
            }
            st.cooldown_until = None;
            st.fail_count = 0;
            st.window_start = now;
        }
        if now.duration_since(st.window_start) > cfg.window {
            st.window_start = now;
            st.fail_count = 0;
        }
        st.fail_count = st.fail_count.saturating_add(1);
        if st.fail_count >= cfg.threshold {
            st.cooldown_until = Some(now + cfg.cooldown);
        }
        self.prune(now);
    }

    /// 표가 문턱을 넘고 마지막 순회로부터 간격이 지났으면, 쿨다운도 없고 윈도우도
    /// 만료된 엔트리를 걷어낸다.
    ///
    /// 크기와 간격은 순회 비용을 제한하는 조건이다. 조기 반환하더라도 표 안에
    /// 만료된 항목이 없다는 뜻은 아니다.
    fn prune(&mut self, now: Instant) {
        if self.sources.len() <= PRUNE_TRIGGER_SOURCES {
            return;
        }
        if let Some(last) = self.last_prune
            && now.duration_since(last) < prune_interval(self.config)
        {
            return;
        }
        self.last_prune = Some(now);
        let window = self.config.window;
        self.sources.retain(|_, st| {
            let cooling = st.cooldown_until.map(|u| now < u).unwrap_or(false);
            cooling || now.duration_since(st.window_start) <= window
        });
    }
}

static TRACKER: OnceLock<Mutex<AbuseTracker>> = OnceLock::new();

fn tracker() -> &'static Mutex<AbuseTracker> {
    TRACKER.get_or_init(|| Mutex::new(AbuseTracker::new(AbuseConfig::from_env())))
}

const TRACKER_WHAT: &str = "the webhook abuse tracker";
static TRACKER_POISON_REPORTED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// poison을 보고하고 추적기를 복구한다. 카운터의 정확성까지 보장하지는 않는다.
/// 락을 인자로 받아 시험이 전역 추적기를 poison하지 않도록 한다.
fn lock_tracker(tracker: &Mutex<AbuseTracker>) -> MutexGuard<'_, AbuseTracker> {
    tasty_utils::poison::recover_mutex(tracker.lock(), TRACKER_WHAT, &TRACKER_POISON_REPORTED)
}

fn lock() -> MutexGuard<'static, AbuseTracker> {
    lock_tracker(tracker())
}

/// 전역 진입점 — 이 출처가 현재 쿨다운(즉시 429) 대상인가.
pub fn is_source_blocked(source: &str) -> bool {
    lock().is_blocked(source, Instant::now())
}

/// 전역 진입점 — 실패 1회를 이 출처로 집계한다([`counts_as_failure`] 로 먼저 거른다).
pub fn record_failure(source: &str) {
    lock().record_failure(source, Instant::now());
}

/// 출처별 실패 집계 대상. 등록별 횟수 제한과는 다르다.
/// 인증 실패(401)는 등록 횟수를 차감하지 않지만 반복 토큰 추측은 여기서 센다.
/// 큰 body(413)도 반복 요청 비용을 제한하기 위해 센다.
/// 정상 접수·만료된 등록·이미 차단된 출처의 응답은 제외한다.
/// 새 ACK가 생기면 집계 여부를 검토하도록 match를 모두 열거한다.
pub fn counts_as_failure(status: AckStatus) -> bool {
    match status {
        AckStatus::NotFound
        | AckStatus::MethodNotAllowed
        | AckStatus::Unauthorized
        | AckStatus::PayloadTooLarge => true,
        AckStatus::Received | AckStatus::Gone | AckStatus::TooManyRequests => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_ack_status_has_a_stated_answer() {
        assert!(counts_as_failure(AckStatus::NotFound));
        assert!(counts_as_failure(AckStatus::MethodNotAllowed));
        assert!(counts_as_failure(AckStatus::Unauthorized));
        assert!(counts_as_failure(AckStatus::PayloadTooLarge));
        assert!(!counts_as_failure(AckStatus::Received));
        assert!(!counts_as_failure(AckStatus::Gone));
        assert!(!counts_as_failure(AckStatus::TooManyRequests));
    }

    // 등록 횟수를 보존하는 listener::tests::unauthorized_does_not_consume_count와 짝을 이룬다.
    #[test]
    fn rejected_tokens_reach_the_cooldown() {
        let mut t = AbuseTracker::new(cfg(3));
        let base = Instant::now();
        for _ in 0..3 {
            t.record_failure("brute", base);
        }
        assert!(
            t.is_blocked("brute", base),
            "틀린 토큰 반복은 쿨다운으로 이어져야 한다"
        );
    }

    fn cfg(threshold: u32) -> AbuseConfig {
        AbuseConfig {
            threshold,
            window: Duration::from_secs(10),
            cooldown: Duration::from_secs(60),
        }
    }

    /// **문턱은 상한이 아니다.** 서로 다른 출처가 같은 윈도우 안에서 실패하면
    /// 모든 항목이 보존 조건을 만족하므로 `prune`은 하나도 지우지 않는다.
    /// 표 크기가 정리 문턱으로 제한되지 않는 사례를 확인한다(ADR-0032).
    #[test]
    fn the_table_grows_past_the_prune_trigger() {
        let mut t = AbuseTracker::new(cfg(20));
        let base = Instant::now();
        let n = PRUNE_TRIGGER_SOURCES + 1000;
        for i in 0..n as u32 {
            t.record_failure(
                &format!("10.{}.{}.{}", i / 65536, (i / 256) % 256, i % 256),
                base,
            );
        }
        assert_eq!(
            t.sources.len(),
            n,
            "시간 범위 안의 항목은 정리 기준 개수를 넘어도 유지한다"
        );
    }

    /// 표가 문턱보다 크고 순회 간격도 지난 상태에서 새 실패가 오면 만료 항목을 회수한다.
    /// 여기서는 기존 항목이 모두 윈도우 밖이며 쿨다운도 없도록 만든다.
    #[test]
    fn an_idle_table_is_reclaimed_by_the_next_failure() {
        let mut t = AbuseTracker::new(cfg(20));
        let base = Instant::now();
        for i in 0..(PRUNE_TRIGGER_SOURCES + 100) as u32 {
            t.record_failure(
                &format!("10.{}.{}.{}", i / 65536, (i / 256) % 256, i % 256),
                base,
            );
        }
        t.record_failure("172.16.0.1", base + Duration::from_secs(11));
        assert_eq!(
            t.sources.len(),
            1,
            "크기·간격 조건을 충족한 순회에서 만료 항목을 제거해야 한다"
        );
    }

    #[test]
    fn a_cooling_entry_survives_a_prune() {
        let mut t = AbuseTracker::new(cfg(2));
        let base = Instant::now();
        t.record_failure("cool", base);
        t.record_failure("cool", base);
        let later = base + Duration::from_secs(11);
        for i in 0..(PRUNE_TRIGGER_SOURCES + 100) as u32 {
            t.record_failure(
                &format!("11.{}.{}.{}", i / 65536, (i / 256) % 256, i % 256),
                later,
            );
        }
        assert!(
            t.is_blocked("cool", later),
            "차단 중인 항목을 제거하면 안 된다"
        );
    }

    #[test]
    fn the_walk_runs_at_most_once_per_window() {
        let mut t = AbuseTracker::new(cfg(20));
        let base = Instant::now();
        for i in 0..(PRUNE_TRIGGER_SOURCES + 1) as u32 {
            t.record_failure(
                &format!("10.{}.{}.{}", i / 65536, (i / 256) % 256, i % 256),
                base,
            );
        }
        assert_eq!(
            t.last_prune,
            Some(base),
            "크기 조건을 충족하면 첫 정리를 수행해야 한다"
        );

        t.record_failure("172.16.0.1", base + Duration::from_secs(5));
        assert_eq!(
            t.last_prune,
            Some(base),
            "정리 간격 안에서는 다시 순회하지 않는다"
        );

        let after = base + Duration::from_secs(11);
        t.record_failure("172.16.0.2", after);
        assert_eq!(
            t.last_prune,
            Some(after),
            "정리 간격이 지나면 다시 순회한다"
        );
    }

    #[test]
    fn trips_cooldown_at_threshold() {
        let base = Instant::now();
        let mut t = AbuseTracker::new(cfg(3));
        assert!(!t.is_blocked("1.2.3.4", base));

        t.record_failure("1.2.3.4", base);
        t.record_failure("1.2.3.4", base);
        assert!(!t.is_blocked("1.2.3.4", base));

        t.record_failure("1.2.3.4", base); // 3회째 → 임계치 도달, 쿨다운.
        assert!(t.is_blocked("1.2.3.4", base));
    }

    #[test]
    fn cooldown_expires_and_source_recovers() {
        let base = Instant::now();
        let mut t = AbuseTracker::new(cfg(2));
        t.record_failure("9.9.9.9", base);
        t.record_failure("9.9.9.9", base);
        assert!(t.is_blocked("9.9.9.9", base));
        assert!(!t.is_blocked("9.9.9.9", base + Duration::from_secs(61)));
    }

    #[test]
    fn a_cooldown_is_not_extended_by_more_failures() {
        let base = Instant::now();
        let mut t = AbuseTracker::new(cfg(2));
        t.record_failure("loud", base);
        t.record_failure("loud", base);
        assert!(t.is_blocked("loud", base));
        for i in 1..=5 {
            t.record_failure("loud", base + Duration::from_secs(30 + i));
        }
        assert!(
            !t.is_blocked("loud", base + Duration::from_secs(61)),
            "추가 실패가 60s 차단 시간을 연장하면 안 된다"
        );
    }

    #[test]
    fn an_expired_cooldown_starts_from_a_clean_count() {
        let base = Instant::now();
        let mut t = AbuseTracker::new(cfg(3));
        for _ in 0..3 {
            t.record_failure("back", base);
        }
        let after = base + Duration::from_secs(61);
        // 실제 리스너처럼 차단 여부를 먼저 조회해 만료 상태를 해제한다.
        assert!(!t.is_blocked("back", after));
        t.record_failure("back", after);
        assert!(
            !t.is_blocked("back", after),
            "해제 뒤 첫 실패 한 건으로 다시 차단되면 안 된다(임계치 3 이 새로 필요)"
        );
    }

    #[test]
    fn normal_source_never_blocked() {
        let base = Instant::now();
        let mut t = AbuseTracker::new(cfg(3));
        t.record_failure("bad", base);
        t.record_failure("bad", base);
        t.record_failure("bad", base);
        assert!(t.is_blocked("bad", base));
        assert!(!t.is_blocked("good", base));
    }

    #[test]
    fn window_resets_scattered_failures() {
        let base = Instant::now();
        let mut t = AbuseTracker::new(cfg(3));
        t.record_failure("slow", base); // count=1
        t.record_failure("slow", base + Duration::from_secs(11));
        t.record_failure("slow", base + Duration::from_secs(12));
        assert!(!t.is_blocked("slow", base + Duration::from_secs(12)));
    }

    // 다른 시험에 영향을 주지 않도록 전역 대신 지역 Mutex를 poison한다.
    #[test]
    fn a_poisoned_tracker_keeps_judging_and_says_so_once() {
        let shared = std::sync::Arc::new(Mutex::new(AbuseTracker::new(cfg(2))));

        let poisoner = std::sync::Arc::clone(&shared);
        std::thread::spawn(move || {
            let _guard = poisoner.lock().expect("아직 성한 락");
            panic!("이 스레드가 락을 쥔 채 죽는다");
        })
        .join()
        .expect_err("패닉한 스레드는 Err 로 join 된다");
        assert!(shared.lock().is_err(), "poison 이 실제로 걸려야 한다");

        let base = Instant::now();
        lock_tracker(&shared).record_failure("9.9.9.9", base);
        lock_tracker(&shared).record_failure("9.9.9.9", base);
        assert!(
            lock_tracker(&shared).is_blocked("9.9.9.9", base),
            "poison 뒤에도 임계치에 도달하면 차단해야 한다"
        );

        assert!(
            TRACKER_POISON_REPORTED.load(std::sync::atomic::Ordering::Relaxed),
            "poison 복구 사실을 한 번은 보고해야 한다"
        );
    }

    #[test]
    fn from_env_defaults_when_unset() {
        // 프로세스 환경을 바꾸지 않고 기본값만 대조한다.
        let d = AbuseConfig::default();
        assert_eq!(d.threshold, 20);
        assert_eq!(d.window, Duration::from_secs(10));
        assert_eq!(d.cooldown, Duration::from_secs(60));
    }
}
