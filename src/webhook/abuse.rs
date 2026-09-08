//! 웹훅 남용 차단 — 매칭·인증 실패 반복 출처 임계치 초과 시 일시 거부(429).
//!
//! opaque 짧은해시 URL 은 keyspace 스캔에 대한 1차 방어지만, 무차별 요청이
//! 리스너/실행 경로를 소모하지 못하도록 **출처(IP)별 실패 카운터 + 쿨다운**을 둔다.
//!
//! ## 정책
//! - 무엇이 실패인가는 [`counts_as_failure`] 하나가 정한다 — 404·405·401 을 세고
//!   200·410·429 는 안 센다. 정상 매칭(200)은 집계 대상이 아니므로 **정상 웹훅
//!   트래픽은 영향받지 않는다.**
//! - 한 출처가 `window` 안에 `threshold` 회 이상 실패하면 `cooldown` 동안 쿨다운
//!   상태가 되고, 이후 그 출처의 요청은 **매칭 전에 즉시 429** 로 거부된다.
//! - 임계치/윈도우/쿨다운은 설정값이며 env 로 오버라이드한다.
//!
//! 모든 시각 판정은 `now: Instant` 를 인자로 받는 순수 코어(`AbuseTracker`)에
//! 모아 테스트가 시간을 통제할 수 있게 한다. 전역 진입점은 `Instant::now()` 를 쓴다.
//!
//! 이 모듈이 담은 명부 열하나 중 **아홉이 결정으로 적혀 있다** — 집계 대상
//! ([ADR-0195](../../docs/adr/0195-abuse-counting-includes-rejected-tokens.md)),
//! 문턱값과 출처 키
//! ([ADR-0196](../../docs/adr/0196-abuse-thresholds-and-source-key.md)), 표의 크기 문턱과
//! 순회 간격([ADR-0197](../../docs/adr/0197-the-source-table-cap-is-a-prune-trigger.md)),
//! 쿨다운의 시간 의미론 셋
//! ([ADR-0198](../../docs/adr/0198-a-cooldown-is-fixed-at-entry.md)), 차단 판정이 body
//! 읽기보다 앞이라는 것
//! ([ADR-0199](../../docs/adr/0199-the-block-is-decided-before-the-body-is-read.md)),
//! 락 poison 복구([ADR-0177](../../docs/adr/0177-recovery-forbidden-locks-are-judged-by-frame-boundary-type.md)).
//!
//! **관찰로 남은 것은 둘**이고, 값이 아니라 표면의 *존재*라 바뀔 때 판단이 갈리지
//! 않는다. 다만 언제 다시 보는지는 적어 둔다.
//!
//! - **env 오버라이드 3 종의 이름** — 이름을 바꾸거나 없애면 사용자가 걸어 둔 설정이
//!   조용히 무시된다(파싱 실패와 미설정이 같은 값으로 떨어진다). 그 표면이 설정 파일
//!   같은 다른 채널로 옮겨가면 그때 결정으로 올린다.
//! - **카운터가 in-memory 라 재시작하면 쿨다운이 사라지는 것** — 지금은 그것이
//!   `webhooks.toml` 에 남는 등록과 대비되는 사실로 문서에만 있다. 차단 상태를 재시작
//!   너머로 잇자는 요구가 나오면(또는 그 소멸이 운영에서 문제로 관측되면) 그때 올린다 —
//!   영속화는 표 크기·회수 규칙(ADR-0197)과 함께 봐야 하는 변경이다.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use super::ack::AckStatus;

/// 출처 표를 정리해 볼 **크기 문턱** — 이 아래에서는 순회를 아예 안 한다.
///
/// **상한이 아니다.** `prune` 은 쿨다운 중이거나 윈도우 안인 엔트리를 보존하는데,
/// 서로 다른 출처가 윈도우 안에서 한꺼번에 실패하면 **전부 그 조건을 만족**해 하나도
/// 안 지워진다 — 표는 이 값을 넘어 계속 자란다(실측: 윈도우 안 10,000 출처 → 길이
/// 10,000). 표의 실제 크기를 정하는 것은 이 값이 아니라 **유입률 × 윈도우**이고,
/// 유입이 멎으면 다음 실패 한 건이 전부 회수한다.
///
/// 값의 근거와 그때의 비용 실측은
/// [ADR-0197](../../docs/adr/0197-the-source-table-cap-is-a-prune-trigger.md).
const PRUNE_TRIGGER_SOURCES: usize = 4096;

/// `prune` 순회의 **최소 간격**. 문턱을 넘은 뒤에도 순회는 윈도우당 한 번만 돈다.
///
/// 없으면 문턱 초과 구간에서 실패 한 건마다 표 전체를 훑는다 — 그리고 그 순회는
/// **아무것도 못 지우면서** 반복된다(위 참조). 그 비용이 전역 락 안에서 나므로 웹훅
/// 처리량 상한이 된다(실측 ADR-0197). 간격을 윈도우로 잡는 이유는 지울 자격이 생기는
/// 단위가 윈도우이기 때문이다 — 그보다 자주 훑어도 새로 지울 것이 없다.
///
/// 회수가 최대 윈도우 하나만큼 늦어진다. 표가 그 시간 동안 유입률 × 윈도우 규모로
/// 남는 것은 간격이 없을 때와 같다.
fn prune_interval(cfg: AbuseConfig) -> Duration {
    cfg.window
}

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

/// 기본값의 근거와 그 값이 지키는 것(짧은 토큰의 무차별 대입)은
/// [ADR-0196](../../docs/adr/0196-abuse-thresholds-and-source-key.md). **같은 값이
/// 사용자 문서 세 자리에 그대로 박혀 있다** — 바꾸면 거기까지 같은 커밋에서 고친다.
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
    /// 마지막으로 `prune` 순회를 **실제로 돈** 시각. `None` 이면 아직 한 번도 안 돌았다
    /// — 시각은 주입받는 값이라 생성자가 `Instant::now()` 를 부르지 않는다.
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
            // 쿨다운 만료 → 초기화하고 통과시킨다.
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

    /// 표가 문턱을 넘고 마지막 순회로부터 간격이 지났으면, 쿨다운도 없고 윈도우도
    /// 만료된 엔트리를 걷어낸다.
    ///
    /// 두 갈래 모두 **조기 반환이 정상 경로**다 — 문턱 아래에서는 지울 것이 없고,
    /// 간격 안에서는 지난번에 이미 훑어 새로 자격이 생긴 것이 없다.
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

/// 남용 추적기 락. 두 전역 진입점이 이 한 곳을 지난다.
///
/// poison 이면 복구한다. ① 임계구역은 `sources` 맵 조작과 saturating 산술뿐이라 최악의
/// 손상이 "한 출처의 실패 카운트가 어긋난 것" 으로 갇힌다. ② 패닉하면 그 요청을 처리하던
/// 리스너 스레드가 죽는다. 조용한 복구도 답이 아니다 — 여기는 차단 판정 경로라, 카운트가
/// 어긋나 차단이 안 걸리거나 과하게 걸려도 그 사실이 어디에도 안 남는다.
///
/// 락을 인자로 받는 이유는 [`registry::lock_state`](super::registry) 와 같다.
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

/// 이 ACK 상태를 **출처 실패로 집계하는가**. 무엇이 남용인가는 남용차단의 정책이라
/// 리스너의 인라인 조건이 아니라 여기 하나가 정한다.
///
/// **통이 둘이면 답도 둘이다.** 등록별 호출 예산(`CountLimit`)에서 401 은 세면 안
/// 된다 — 그 통이 세는 단위는 "시퀀스를 돌린 횟수" 이고 401 은 그것을 0 번 돌리며,
/// 통의 키가 토큰이 아니라 등록이라 세면 익명 발신자가 owner 의 예산을 태운다. 이
/// 통은 반대다. 제한이라 **안 세는 것이 우회**이고, 401 은 opaque path 를 이미 맞춘
/// 발신자가 비밀을 무차별 대입하는 자리다 — 통 A 도 안 태우므로 여기서 세지 않으면
/// 그 대입에 붙는 비용이 어디에도 없다([ADR-0195](../../docs/adr/0195-abuse-counting-includes-rejected-tokens.md)).
///
/// `PayloadTooLarge`(413)도 센다 — 상한을 넘는 body 를 반복해 보내는 것은 그 자체가
/// 자원을 겨눈 요청이고, 그 요청도 아무것도 얻지 못하고 끝난다. 상한 덕에 한 건의 값은
/// 묶였지만 반복 횟수는 안 묶인다([ADR-0200](../../docs/adr/0200-webhook-body-has-a-per-request-byte-cap.md)).
///
/// 나머지 셋은 그 물음에 답이 다르다.
/// - `Received`(200) — 정상 트래픽. 세면 남용차단이 정상 발신자를 막는다.
/// - `Gone`(410) — 만료된 등록. path 를 맞춰야 나오지만 **추측할 공간이 없다**(같은
///   URL 을 몇 번 두드려도 얻는 정보가 0). 레이트 제한의 대상은 시도가 정보를 주는
///   자리다.
/// - `TooManyRequests`(429) — 이미 쿨다운 중이라 매칭 전에 거부돼 이 자리에 닿지도
///   않고, 센다면 쿨다운이 스스로 연장된다.
///
/// 와일드카드를 쓰지 않는다 — 새 ACK 상태가 생기면 그 자리에서 컴파일이 멈춰야
/// 한다. 이 게이트가 조용히 빠뜨리는 것이 곧 우회다.
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

    /// 어떤 ACK 가 이 통을 세는지 **전수로** 못박는다. 게이트가 조용히 빠뜨리는 것이
    /// 곧 우회이고, 그 빠짐은 variant 를 하나 더할 때 가장 쉽게 생긴다.
    #[test]
    fn every_ack_status_has_a_stated_answer() {
        // 센다 — 셋 다 "요청이 아무것도 못 얻고 끝난" 자리이고, 반복이 곧 탐색이다.
        assert!(counts_as_failure(AckStatus::NotFound));
        assert!(counts_as_failure(AckStatus::MethodNotAllowed));
        assert!(counts_as_failure(AckStatus::Unauthorized));
        assert!(counts_as_failure(AckStatus::PayloadTooLarge));
        // 안 센다 — 정상 트래픽 / 추측 공간 없음 / 이미 쿨다운(세면 자기 연장).
        assert!(!counts_as_failure(AckStatus::Received));
        assert!(!counts_as_failure(AckStatus::Gone));
        assert!(!counts_as_failure(AckStatus::TooManyRequests));
    }

    /// 401 은 통 A(등록별 예산)를 안 태우고 통 B(이 통)는 태운다. 두 술어가 401 에
    /// 대해 **반대 답**을 준다는 것이 이 설계의 요지라, 한쪽만 보면 다음 사람이
    /// "401 은 아무 데도 안 센다" 로 읽는다 — 통 A 쪽 짝은
    /// `listener::tests::unauthorized_does_not_consume_count` 다.
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

    /// **문턱은 상한이 아니다.** 서로 다른 출처가 윈도우 안에서 실패하면 `prune` 이
    /// 보존 조건(쿨다운 중 · 윈도우 안)을 모두 만족시켜 하나도 못 지우고, 표는 문턱을
    /// 넘어 계속 자란다. 이름이 상한처럼 읽혀 온 자리라 값으로 못박는다 — 표 크기를
    /// 정하는 것은 이 문턱이 아니라 유입률 × 윈도우다(ADR-0197).
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
            "윈도우 안 엔트리는 문턱을 넘어도 안 밀린다"
        );
    }

    /// 유입이 멎으면 표는 회수된다 — 윈도우 밖이 된 뒤 실패 **한 건**이 전부 걷어낸다.
    /// 위 테스트와 짝이다: 상한이 없다는 것이 곧 누수라는 뜻은 아니다.
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
            "윈도우 밖 엔트리는 다음 실패 한 건이 회수한다"
        );
    }

    /// 쿨다운 중인 엔트리는 순회를 견딘다 — 새 출처를 뿌려 자기 차단을 밀어낼 수 없다.
    /// 이것이 없으면 표를 넘치게 하는 것 자체가 차단 해제 수단이 된다.
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
            "쿨다운 엔트리가 축출되면 안 된다"
        );
    }

    /// 문턱을 넘은 뒤에도 순회는 **윈도우당 한 번**만 돈다. 없으면 실패 한 건마다 표
    /// 전체를 훑고, 그 순회는 지울 것이 없는 동안에도 반복된다(비용 실측은 ADR-0197).
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
            "문턱을 넘은 그 자리에서 한 번은 돈다"
        );

        t.record_failure("172.16.0.1", base + Duration::from_secs(5));
        assert_eq!(t.last_prune, Some(base), "간격 안에서는 다시 안 돈다");

        let after = base + Duration::from_secs(11);
        t.record_failure("172.16.0.2", after);
        assert_eq!(t.last_prune, Some(after), "간격이 지나면 다시 돈다");
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

    /// **쿨다운은 연장되지 않는다** — 차단 중에 더 두드려도 만료 시각은 진입 때 정해진
    /// 값 그대로다. 연장하면 계속 두드리는 발신자가 사실상 영구 차단되고, 그 발신자가
    /// 토큰을 잘못 설정한 정상 발신자일 때 스스로 빠져나올 길이 사라진다(ADR-0198).
    #[test]
    fn a_cooldown_is_not_extended_by_more_failures() {
        let base = Instant::now();
        let mut t = AbuseTracker::new(cfg(2));
        t.record_failure("loud", base);
        t.record_failure("loud", base);
        assert!(t.is_blocked("loud", base));
        // 차단 중 재실패 — 만료 시각을 밀지 못한다.
        for i in 1..=5 {
            t.record_failure("loud", base + Duration::from_secs(30 + i));
        }
        assert!(
            !t.is_blocked("loud", base + Duration::from_secs(61)),
            "쿨다운 60s 는 진입 시각이 정한다 — 재실패가 밀지 않는다"
        );
    }

    /// 쿨다운이 풀리면 **카운터도 백지**가 된다. 안 그러면 해제 직후 실패 한 건이 곧장
    /// 임계치를 다시 채워, 연장을 막아 둔 것이 의미를 잃는다(ADR-0198).
    #[test]
    fn an_expired_cooldown_starts_from_a_clean_count() {
        let base = Instant::now();
        let mut t = AbuseTracker::new(cfg(3));
        for _ in 0..3 {
            t.record_failure("back", base);
        }
        let after = base + Duration::from_secs(61);
        // 실제 경로와 같은 순서 — 리스너는 매 요청 `is_blocked` 를 먼저 묻는다.
        assert!(!t.is_blocked("back", after));
        t.record_failure("back", after);
        assert!(
            !t.is_blocked("back", after),
            "해제 뒤 첫 실패 한 건으로 다시 차단되면 안 된다(임계치 3 이 새로 필요)"
        );
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

    /// poison 이 걸린 뒤에도 차단 판정이 계속 돌고, 그 사실이 한 번은 남는가.
    ///
    /// 전역이 아니라 지역 뮤텍스를 겨냥한다 — 전역을 poison 하면 같은 테스트
    /// 바이너리의 뒤 테스트가 그 상태를 물려받는다.
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

        // 판정이 계속 돈다 — 임계치까지 집계하고 차단으로 넘어간다.
        let base = Instant::now();
        lock_tracker(&shared).record_failure("9.9.9.9", base);
        lock_tracker(&shared).record_failure("9.9.9.9", base);
        assert!(
            lock_tracker(&shared).is_blocked("9.9.9.9", base),
            "poison 뒤에도 임계치를 넘으면 차단해야 한다"
        );

        assert!(
            TRACKER_POISON_REPORTED.load(std::sync::atomic::Ordering::Relaxed),
            "복구했으면 한 번은 보고해야 한다 — 조용한 복구는 조용한 오판과 구분되지 않는다"
        );
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
