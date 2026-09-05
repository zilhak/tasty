//! 지연 단정의 **대조군**.
//!
//! `assert!(elapsed < LIMIT)` 한 줄은 부하가 만든 값과 코드가 만든 값을 구분하지
//! 못한다. 이 크레이트는 그 자리에 **같은 자원을 지나되 측정 대상의 코드 경로는
//! 지나지 않는 값**을 하나 더 실어서, 실패 문장이 둘 중 어느 쪽인지 스스로 적게
//! 한다. 선택 규칙·거부한 대안·재검토 조건은
//! `docs/adr/0181-a-latency-assertion-must-carry-a-control-that-load-moves-and-code-does-not.md`.
//!
//! **계열을 자리마다 고른다.** 측정 대상이 CPU 를 기다리면 스케줄러 계열
//! ([`ControlProbe::start`]), 자식 프로세스를 띄우고 기다리면 spawn 계열
//! ([`ControlProbe::start_spawn`]). 실측으로 writeback 이 포화돼 IPC 왕복이 5.4 초일 때
//! 스케줄러 쪽 지표는 7 ms 밖에 안 움직였다 — 계열을 잘못 고르면 대조군이 "부하 아님"
//! 이라고 **잘못 증언한다**. IPC 왕복처럼 채널 뒤에 줄 서는 값의 대조군은 이 크레이트가
//! 아직 안 준다; 같은 채널의 값싼 왕복이어야 하고 그건 호출자 쪽 물건이다.

// 이유: 이 lint 의 출력은 위반 목록이 아니라 **프로덕션 명부**다
// (docs/dev-guide/error-handling.md). 테스트 자리가 섞이면 새 프로덕션 자리가
// 묻히므로, 자리마다가 아니라 크레이트 루트에서 test 범위를 통째로 덮는다.
#![cfg_attr(test, allow(clippy::let_underscore_must_use))]
use std::time::{Duration, Instant};

/// 대조군이 자기 기준선의 이 배수 이상이면 러너가 굶은 것으로 본다.
///
/// 3 배인 이유: 유휴 상태의 스핀 비용은 15 회 반복에서 최대-최소 차가 2% 였다(실측).
/// 그 변동의 100 배가 넘는 여유라, 이 문턱을 넘었다는 것은 잡음이 아니다. 반대로
/// 이 문턱은 **꾸준한** 부하에는 안 걸린다 — 그런 부하는 기준선에 함께 흡수된다.
pub const INFLATED: f64 = 3.0;

/// 대조군 한 번의 일감 크기. 유휴에서 이 크레이트의 test 프로필 기준선이 412 µs 였고
/// (`-O` 로 재면 38 µs), 실패 경로에서만 도는 값이라 이 비용은 예산에 들어가지 않는다.
const SPIN_ITERS: u64 = 50_000;

/// 계열 이름 — 실패 문장이 어느 대조군을 썼는지 밝힌다.
const CPU_KIND: &str = "고정 CPU 일감";
const SPAWN_KIND: &str = "자식 하나 띄우기";

/// 기준선을 잡을 때 몇 번 재서 **최소**를 고르나. 최소를 쓰는 이유는 여러 번 중
/// 가장 덜 선점된 회차가 유휴 비용에 가장 가깝기 때문이다 — 평균은 그 순간의
/// 부하를 기준선에 섞어 넣어, 부하 속에서 만든 기준선이 부하를 못 보게 만든다.
const CALIBRATION_ROUNDS: usize = 9;

/// spawn 계열은 한 회가 밀리초 단위라 회차를 줄인다. 이 값이 곧 초록 경로에서
/// 띄우는 자식 수라, 대조군 자신이 러너에 부담이 되지 않게 3 으로 둔다.
const SPAWN_CALIBRATION_ROUNDS: usize = 3;

/// 자식 하나를 띄우고 거둘 때까지. 측정 대상이 `Command::spawn` 뒤에 줄 서는 자리
/// (ssh 포트 발견 등)의 대조군이다 — fork/exec·스케줄러·바이너리 적재까지 같은 자원을
/// 지나면서, 측정 대상의 코드는 한 줄도 안 부른다.
///
/// 띄우지 못하면 `Duration::ZERO` 를 준다. 기준선이 0 이면 [`ControlSample::ratio`] 가
/// 1.0 을 주므로, **대조군이 없을 때 부하를 주장하지 않는다**.
fn spawn_probe() -> Duration {
    let started = Instant::now();
    #[cfg(windows)]
    let mut cmd = {
        let mut c = std::process::Command::new("cmd");
        c.args(["/c", "exit"]);
        c
    };
    #[cfg(not(windows))]
    let mut cmd = std::process::Command::new("/bin/true");
    let Ok(mut child) = cmd
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    else {
        return Duration::ZERO;
    };
    if child.wait().is_err() {
        return Duration::ZERO;
    }
    started.elapsed()
}

/// `work` 를 `rounds` 번 재서 최소를 준다.
fn calibrate_with(work: fn() -> Duration, rounds: usize) -> Duration {
    let mut baseline = work();
    for _ in 1..rounds {
        baseline = baseline.min(work());
    }
    baseline
}

/// 고정된 CPU 일감. 최적화로 사라지지 않게 결과를 `black_box` 로 붙잡는다.
fn spin() -> Duration {
    let started = Instant::now();
    let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
    for i in 0..SPIN_ITERS {
        x = x.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(i);
    }
    std::hint::black_box(x);
    started.elapsed()
}

/// 스케줄러 대조군 — 고정된 CPU 일감이 지금 얼마나 걸리나.
///
/// ADR-0181 의 선택 규칙 셋을 이렇게 만족한다: ① 러너가 굶으면 같이 늘어난다
/// (같은 스레드·같은 CPU), ② 측정 대상의 코드를 한 줄도 안 부른다, ③ 싸고 변동이 작다.
#[derive(Debug, Clone, Copy)]
pub struct CpuControl {
    baseline: Duration,
}

impl CpuControl {
    /// 지금 자리의 기준선을 잡는다. **측정 대상을 돌리기 전에** 부른다.
    pub fn calibrate() -> Self {
        Self {
            baseline: calibrate_with(spin, CALIBRATION_ROUNDS),
        }
    }

    /// 기준선을 값으로 준다 — 산술을 부하 없이 고정하는 시험용.
    pub fn with_baseline(baseline: Duration) -> Self {
        Self { baseline }
    }

    pub fn baseline(&self) -> Duration {
        self.baseline
    }

    /// 지금 한 번 재서 기준선과 비교한다.
    pub fn sample(&self) -> ControlSample {
        ControlSample {
            cost: spin(),
            baseline: self.baseline,
            kind: CPU_KIND,
        }
    }
}

/// 대조군 한 번의 결과.
#[derive(Debug, Clone, Copy)]
pub struct ControlSample {
    cost: Duration,
    baseline: Duration,
    /// 어느 계열의 대조군인가 — 문장이 이것을 밝혀야 읽는 사람이 "그 대조군이 이 자리에
    /// 맞나" 를 다시 물을 수 있다. 계열을 잘못 고르는 것이 이 설계의 주된 사고다.
    kind: &'static str,
}

impl ControlSample {
    /// 잰 값으로 만든다 — 산술을 부하 없이 고정하는 시험용.
    pub fn from_parts(cost: Duration, baseline: Duration) -> Self {
        Self::from_parts_with_kind(CPU_KIND, cost, baseline)
    }

    pub fn from_parts_with_kind(kind: &'static str, cost: Duration, baseline: Duration) -> Self {
        Self {
            cost,
            baseline,
            kind,
        }
    }

    pub fn kind(&self) -> &'static str {
        self.kind
    }

    pub fn cost(&self) -> Duration {
        self.cost
    }

    pub fn baseline(&self) -> Duration {
        self.baseline
    }

    /// 기준선 대비 배수. 기준선이 0 이면 비교가 성립하지 않으므로 1.0 을 준다
    /// (= "부풀지 않았다") — 없는 근거로 부하를 주장하지 않는다.
    pub fn ratio(&self) -> f64 {
        let base = self.baseline.as_secs_f64();
        if base <= 0.0 {
            return 1.0;
        }
        self.cost.as_secs_f64() / base
    }

    pub fn is_inflated(&self) -> bool {
        self.ratio() >= INFLATED
    }
}

/// 지연 단정이 실패할 때 실을 문장. **두 사건을 가른다.**
pub fn latency_verdict(
    what: &str,
    measured: Duration,
    limit: Duration,
    sample: &ControlSample,
) -> String {
    let ratio = sample.ratio();
    let kind = sample.kind;
    if sample.is_inflated() {
        return format!(
            "{what} 이 {measured:?} 걸려 상한 {limit:?} 를 넘었다 — 그런데 대조군({kind})도 \
             기준선의 {ratio:.1} 배로 부풀었다({:?} → {:?}). 러너가 굶은 것이라 \
             이 빨강은 코드에 대한 증거가 아니다. 상한 인상은 이 사건의 처방이 아니다",
            sample.baseline, sample.cost,
        );
    }
    format!(
        "{what} 이 {measured:?} 걸려 상한 {limit:?} 를 넘었다. 대조군({kind})은 기준선의 \
         {ratio:.1} 배로 정상이다({:?} → {:?}) — 러너가 아니라 측정 대상 자신이 느려졌다",
        sample.baseline, sample.cost,
    )
}

/// 대조군을 들고 있다가, 어느 경로로 빠져나가든 값을 남긴다.
///
/// 이 헬퍼의 존재 이유가 "지연 단정이 실패할 때 대조군 값을 남기는 것" 인데,
/// 측정 도중 **다른** 단정이 먼저 패닉하면 그 값이 통째로 사라진다. 그래서 출력을
/// `Drop` 에 둔다 — 되감기(unwind) 중에도 값이 나온다. 초록일 때는 아무 말도 안 한다.
pub struct ControlProbe {
    label: String,
    work: fn() -> Duration,
    kind: &'static str,
    baseline: Duration,
    reported: bool,
}

impl ControlProbe {
    /// 스케줄러 계열. **측정 대상을 돌리기 전에** 만든다 — 기준선이 측정 전 상태를
    /// 담아야 한다. 측정 대상이 CPU·락만 기다리는 자리에 쓴다.
    pub fn start(label: impl Into<String>) -> Self {
        Self::with(label, spin, CALIBRATION_ROUNDS, CPU_KIND)
    }

    /// 프로세스 spawn 계열. 측정 대상이 자식을 띄우고 기다리는 자리에 쓴다 —
    /// 스케줄러 계열은 fork/exec·바이너리 적재의 포화를 못 본다.
    pub fn start_spawn(label: impl Into<String>) -> Self {
        Self::with(label, spawn_probe, SPAWN_CALIBRATION_ROUNDS, SPAWN_KIND)
    }

    fn with(
        label: impl Into<String>,
        work: fn() -> Duration,
        rounds: usize,
        kind: &'static str,
    ) -> Self {
        Self {
            label: label.into(),
            work,
            kind,
            baseline: calibrate_with(work, rounds),
            reported: false,
        }
    }

    fn sample(&self) -> ControlSample {
        ControlSample {
            cost: (self.work)(),
            baseline: self.baseline,
            kind: self.kind,
        }
    }

    /// 실패 문장을 만든다. 이 값이 패닉 메시지에 실리므로 `Drop` 은 입을 다문다.
    pub fn verdict(&mut self, measured: Duration, limit: Duration) -> String {
        self.reported = true;
        let sample = self.sample();
        latency_verdict(&self.label, measured, limit, &sample)
    }
}

/// `Drop` 이 입을 여는 조건. 순수 함수로 빼둔 이유는 이 조건 자신이 R445 의 내용이라
/// 시험 대상이어야 하기 때문이다 — 초록일 때 떠들면 13 자리가 매 실행마다 잡음을 내고,
/// 패닉일 때 입을 다물면 이 크레이트가 존재할 이유가 없어진다.
fn drop_should_speak(reported: bool, panicking: bool) -> bool {
    !reported && panicking
}

impl Drop for ControlProbe {
    fn drop(&mut self) {
        if !drop_should_speak(self.reported, std::thread::panicking()) {
            return;
        }
        let sample = self.sample();
        eprintln!(
            "[대조군 {} · {}] 이 패닉은 지연 단정이 낸 것이 아니다 — 대조군은 기준선의 {:.1} 배다({:?} → {:?}). \
             3 배 이상이면 러너가 굶은 것이고, 그때 위 실패의 원인 지목을 믿으면 안 된다.",
            self.label,
            self.kind,
            sample.ratio(),
            sample.baseline(),
            sample.cost(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ★ ADR-0181 의 규칙 2 를 재는 변이 단정이다(R437).
    ///
    /// 측정 대상 경로에 인위적 지연을 넣는다 — 여기서는 "그 경로가 막혀 있다" 를 잠으로
    /// 흉내낸다. 대조군이 그 경로를 지나면 기준선(`calibrate` 가 잰 순수 스핀) 대비
    /// 비율이 통째로 튄다. 이 단정이 없으면 "대조군은 코드에 반응하지 않는다" 는
    /// **가정**이고, 그때 이 설계는 지금(거짓 양성)보다 나쁜 **거짓 음성**을 만든다.
    ///
    /// **앞뒤 차분으로는 이 변이를 못 잡는다** — 대조군이 그 경로를 지나면 앞 표본도
    /// 같이 부풀어 비가 상쇄된다. 그래서 기준선 대비 절대값으로 잰다.
    #[test]
    fn an_artificial_delay_in_the_measured_path_does_not_move_the_control() {
        let control = CpuControl::calibrate();
        let before = control.sample();
        std::thread::sleep(Duration::from_millis(200));
        let after = control.sample();

        assert!(
            !after.is_inflated(),
            "{}",
            control_independence_verdict(&before, &after)
        );
    }

    /// 이 시험의 빨강도 두 사건을 가려야 한다.
    ///
    /// ★ **정상 부하는 여기 안 걸린다** — 꾸준한 굶주림은 기준선(`calibrate`)에 그대로
    /// 흡수돼 비율이 1 근처로 나온다. 그래서 이 시험이 빨간 것은 "부하가 세다" 가 아니라
    /// "보정 뒤에 무언가가 달라졌다" 는 뜻이고, 그 무언가가 어디서 왔는지를 앞 표본이 가른다.
    fn control_independence_verdict(before: &ControlSample, after: &ControlSample) -> String {
        if before.is_inflated() {
            return format!(
                "보정 **직후** 첫 표본이 이미 기준선의 {:.1} 배다({:?} → {:?}). 그 사이에는 \
                 아무것도 안 끼웠으므로 굶주림으로 설명되지 않는다 — 보정이 도는 일감과 표본이 \
                 도는 일감이 서로 다르다. 대조군 자신이 깨진 것이고, 이 상태의 비율은 부하 \
                 판정에 못 쓴다",
                before.ratio(),
                before.baseline(),
                before.cost(),
            );
        }
        format!(
            "지연을 넣기 전 대조군은 기준선의 {:.1} 배로 정상이었는데, 측정 대상 경로에 200 ms \
             지연을 넣은 뒤 {:.1} 배가 됐다({:?}). 대조군이 그 경로를 지난다 — ADR-0181 규칙 2 \
             위반이고, 이 상태에서는 진짜 회귀가 '부하였다' 로 덮인다. ★ 다만 이 자리는 \
             '그 200 ms 창에 굶주림이 **시작**한 경우' 와 원리적으로 구분되지 않는다. 조용할 때 \
             한 번 더 재서 재현되면 위반이다",
            before.ratio(),
            after.ratio(),
            after.cost(),
        )
    }

    /// 위 문장이 두 사건을 실제로 가르나 — 양방향.
    #[test]
    fn the_independence_verdict_separates_a_broken_control_from_a_reactive_one() {
        let base = Duration::from_micros(170);

        // 앞 표본이 이미 부풀었다 = 보정과 표본이 다른 일감을 돈다.
        let broken = control_independence_verdict(
            &ControlSample::from_parts(base * 900, base),
            &ControlSample::from_parts(base * 900, base),
        );
        assert!(broken.contains("대조군 자신이 깨진 것"), "{broken}");
        assert!(!broken.contains("규칙 2\n"), "{broken}");
        assert!(!broken.contains("그 경로를 지난다"), "{broken}");

        // 앞은 정상, 뒤만 부풀었다 = 주입한 지연에 반응했다.
        let reactive = control_independence_verdict(
            &ControlSample::from_parts(base, base),
            &ControlSample::from_parts(base * 900, base),
        );
        assert!(reactive.contains("그 경로를 지난다"), "{reactive}");
        assert!(
            reactive.contains("원리적으로 구분되지 않는다"),
            "{reactive}"
        );
        assert!(!reactive.contains("대조군 자신이 깨진 것"), "{reactive}");
    }

    /// spawn 계열도 같은 규칙을 진다 — 측정 대상 경로의 지연에 안 움직여야 한다(R437).
    #[test]
    fn the_spawn_control_is_deaf_to_a_delay_in_the_measured_path() {
        let baseline = calibrate_with(spawn_probe, SPAWN_CALIBRATION_ROUNDS);
        if baseline.is_zero() {
            // 자식을 못 띄우는 환경 — 대조군이 없다. 없는 것을 있다고 말하지 않는다.
            return;
        }
        let before = ControlSample::from_parts_with_kind(SPAWN_KIND, spawn_probe(), baseline);
        std::thread::sleep(Duration::from_millis(200));
        let after = ControlSample::from_parts_with_kind(SPAWN_KIND, spawn_probe(), baseline);
        assert!(
            !after.is_inflated(),
            "{}",
            control_independence_verdict(&before, &after)
        );
    }

    /// 부하 없이 산술만 고정한다 — 양방향.
    #[test]
    fn the_verdict_separates_a_starved_runner_from_a_slow_code_path() {
        let base = Duration::from_micros(170);
        let limit = Duration::from_millis(50);
        let measured = Duration::from_millis(80);

        let starved = ControlSample::from_parts(base * 9, base);
        let msg = latency_verdict("await 즉시 반환", measured, limit, &starved);
        assert!(msg.contains("러너가 굶은 것"), "{msg}");
        assert!(msg.contains("증거가 아니다"), "{msg}");
        assert!(msg.contains("상한 인상은 이 사건의 처방이 아니다"), "{msg}");

        let quiet = ControlSample::from_parts(base * 2, base);
        let msg = latency_verdict("await 즉시 반환", measured, limit, &quiet);
        assert!(msg.contains("자신이 느려졌다"), "{msg}");
        assert!(!msg.contains("러너가 굶은 것"), "{msg}");
        // 계열이 문장에 실린다 — 잘못 고른 대조군을 읽는 사람이 알아볼 수 있어야 한다.
        assert!(msg.contains(CPU_KIND), "{msg}");
        let spawn_msg = latency_verdict(
            "무엇",
            measured,
            limit,
            &ControlSample::from_parts_with_kind(SPAWN_KIND, base * 2, base),
        );
        assert!(spawn_msg.contains(SPAWN_KIND), "{spawn_msg}");
        assert!(!spawn_msg.contains(CPU_KIND), "{spawn_msg}");
    }

    /// 문턱 양쪽 — 3 배 미만은 정상, 3 배는 부하.
    #[test]
    fn the_inflation_threshold_is_a_boundary_not_a_slope() {
        let base = Duration::from_micros(1000);
        assert!(
            !ControlSample::from_parts(base * 3 - Duration::from_micros(1), base).is_inflated()
        );
        assert!(ControlSample::from_parts(base * 3, base).is_inflated());
    }

    /// 기준선이 0 이면 비교가 성립하지 않는다 — 없는 근거로 부하를 주장하지 않는다.
    #[test]
    fn a_zero_baseline_does_not_claim_load() {
        let sample = ControlSample::from_parts(Duration::from_secs(9), Duration::ZERO);
        assert!(!sample.is_inflated());
        let msg = latency_verdict(
            "무엇",
            Duration::from_secs(9),
            Duration::from_secs(1),
            &sample,
        );
        assert!(msg.contains("자신이 느려졌다"), "{msg}");
    }

    /// R445 — 패닉일 때만 입을 연다. 네 조합 전부.
    #[test]
    fn the_probe_speaks_only_on_the_panicking_path() {
        assert!(drop_should_speak(false, true));
        assert!(!drop_should_speak(true, true));
        assert!(!drop_should_speak(false, false));
        assert!(!drop_should_speak(true, false));
    }

    /// 문장을 가져간 뒤에는 `Drop` 이 다시 말하지 않는다.
    #[test]
    fn taking_the_verdict_silences_the_drop() {
        let mut probe = ControlProbe::start("무엇");
        // 문장 자체는 다른 시험이 본다 — 여기서 재는 것은 `verdict` 를 가져간 뒤
        // `Drop` 이 입을 다무는가 하나다.
        let _ = probe.verdict(Duration::from_millis(80), Duration::from_millis(50));
        assert!(!drop_should_speak(probe.reported, true));
    }
}
