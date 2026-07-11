//! 웹훅 남용 차단 — 404/405 반복 출처 임계치 초과 시 일시 거부(429).
//!
//! opaque 짧은해시 URL 은 keyspace 스캔에 대한 1차 방어지만, 무차별 요청이
//! 리스너/실행 경로를 소모하지 못하도록 **출처(IP)별 실패 카운터 + 쿨다운**을 둔다.
//!
//! ## 정책
//! - 매칭 실패(404 NotFound / 405 MethodNotAllowed)만 **출처 실패로 집계**한다.
//!   정상 매칭(200)은 집계 대상이 아니므로 **정상 웹훅 트래픽은 영향받지 않는다.**
//! - 한 출처가 `window` 안에 `threshold` 회 이상 실패하면 `cooldown` 동안 쿨다운
//!   상태가 되고, 이후 그 출처의 요청은 **매칭 전에 즉시 429** 로 거부된다.
//! - 임계치/윈도우/쿨다운은 설정값이며 env 로 오버라이드한다.
//!
//! 모든 시각 판정은 `now: Instant` 를 인자로 받는 순수 코어(`AbuseTracker`)에
//! 모아 테스트가 시간을 통제할 수 있게 한다. 전역 진입점은 `Instant::now()` 를 쓴다.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

/// 출처 상태 맵이 무한정 커지지 않도록 이 크기를 넘으면 만료 엔트리를 정리한다.
const MAX_SOURCES: usize = 4096;

/// 남용 차단 설정값.
#[derive(Debug, Clone, Copy)]
pub struct AbuseConfig {
    /// `window` 안에서 이 횟수 이상 실패하면 쿨다운으로 전환.
    pub threshold: u32,
    /// 실패 카운팅 윈도우. 이 시간이 지나면 카운터가 리셋된다.
    pub window: Duration,
    /// 임계치 초과 시 즉시 거부(429)를 유지하는 시간.
    pub cooldown: Duration,
}

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
    std::env::var(key).ok().and_then(|s| s.trim().parse::<u64>().ok())
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

/// 한 출처의 실패 추적 상태.
#[derive(Debug)]
struct SourceState {
    /// 현재 카운팅 윈도우 시작 시각.
    window_start: Instant,
    /// 현재 윈도우에서 누적된 실패 횟수.
    fail_count: u32,
    /// 쿨다운 종료 시각(설정돼 있으면 그 전까지 즉시 거부).
    cooldown_until: Option<Instant>,
}

/// 출처별 남용 추적기(순수 코어 — 시각은 인자로 주입).
#[derive(Debug)]
pub struct AbuseTracker {
    config: AbuseConfig,
    sources: HashMap<String, SourceState>,
}

impl AbuseTracker {
    pub fn new(config: AbuseConfig) -> Self {
        Self {
            config,
            sources: HashMap::new(),
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
            // 쿨다운 만료 → 초기화하고 통과시킨다.
            st.cooldown_until = None;
            st.fail_count = 0;
            st.window_start = now;
        }
        false
    }

    /// 404/405 실패 1회 기록. 윈도우 내 누적이 임계치에 도달하면 쿨다운 시작.
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
        // 이미 쿨다운 중이면 카운터를 더 굴리지 않는다(연장 방지).
        if let Some(until) = st.cooldown_until {
            if now < until {
                return;
            }
            st.cooldown_until = None;
            st.fail_count = 0;
            st.window_start = now;
        }
        // 윈도우 만료 시 카운터 리셋.
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

    /// 맵이 상한을 넘으면 쿨다운도 없고 윈도우도 만료된 엔트리를 걷어낸다.
    fn prune(&mut self, now: Instant) {
        if self.sources.len() <= MAX_SOURCES {
            return;
        }
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

fn lock() -> MutexGuard<'static, AbuseTracker> {
    tracker().lock().unwrap_or_else(|p| p.into_inner())
}

/// 전역 진입점 — 이 출처가 현재 쿨다운(즉시 429) 대상인가.
pub fn is_source_blocked(source: &str) -> bool {
    lock().is_blocked(source, Instant::now())
}

/// 전역 진입점 — 404/405 실패 1회를 이 출처로 집계한다.
pub fn record_failure(source: &str) {
    lock().record_failure(source, Instant::now());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(threshold: u32) -> AbuseConfig {
        AbuseConfig {
            threshold,
            window: Duration::from_secs(10),
            cooldown: Duration::from_secs(60),
        }
    }

    #[test]
    fn trips_cooldown_at_threshold() {
        let base = Instant::now();
        let mut t = AbuseTracker::new(cfg(3));
        assert!(!t.is_blocked("1.2.3.4", base));

        t.record_failure("1.2.3.4", base);
        t.record_failure("1.2.3.4", base);
        // 임계치 미만 → 아직 통과.
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
        // 쿨다운(60s) 경과 후 해제.
        assert!(!t.is_blocked("9.9.9.9", base + Duration::from_secs(61)));
    }

    #[test]
    fn normal_source_never_blocked() {
        // 실패를 낸 적 없는 출처는 절대 차단되지 않는다(정상 웹훅 무영향).
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
        // 윈도우(10s) 밖 → 카운터 리셋되어 count=1 로 재시작.
        t.record_failure("slow", base + Duration::from_secs(11));
        t.record_failure("slow", base + Duration::from_secs(12));
        // 리셋 이후 2회뿐이라 임계치(3) 미달 → 차단 안 됨.
        assert!(!t.is_blocked("slow", base + Duration::from_secs(12)));
    }

    #[test]
    fn from_env_defaults_when_unset() {
        // env 미설정 기본값 확인(격리를 위해 값 비교만).
        let d = AbuseConfig::default();
        assert_eq!(d.threshold, 20);
        assert_eq!(d.window, Duration::from_secs(10));
        assert_eq!(d.cooldown, Duration::from_secs(60));
    }
}
