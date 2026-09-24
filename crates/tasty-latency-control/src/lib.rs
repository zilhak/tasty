//! 시간 제한 시험이 실패했을 때 함께 기록하는 대조군 측정.
//! CPU·프로세스 실행·채널 왕복은 기다리는 자원이 달라 서로 대신 쓸 수 없다.
//! 대조군은 측정 대상의 코드 경로를 지나지 않아야 한다.
//!
//! spawn과 채널 왕복은 유휴 상태에서도 변동이 커 실제 지연 시험에 적용하지 않는다.
//! 특히 채널 왕복은 측정 대상과 같은 시간 구간에서 표본을 얻는 방법이 필요하다.
//! 자세한 선택 기준은 docs/dev-guide/self-verification.md#시간-측정과-실패-진단 을 따른다.

// 이유: 테스트의 let _ =를 제품 코드의 오류 처리 명부에서 제외한다.
#![cfg_attr(test, allow(clippy::let_underscore_must_use))]
use std::time::{Duration, Instant};

/// 대조군 비율의 정상·판정 불가·부하 증가 구간. 유휴 변동 범위가 불명확한
/// 계열은 starved_at을 None으로 두어 외부 부하로 판정하지 않는다.
#[derive(Debug)]
pub struct Family {
    /// 진단에 표시할 대조군 종류.
    kind: &'static str,
    /// 이 값보다 작으면 대조군 증가가 확인되지 않는다.
    quiet_below: f64,
    /// 부하 증가로 분류할 최소 비율. None이면 그런 판정을 하지 않는다.
    starved_at: Option<f64>,
    /// 측정에서 관측한 유휴 상태의 최대 비율. 다른 환경의 상한은 아니다.
    idle_worst: f64,
}

/// 일감 2_000_000, 200ms 유휴 12회 측정에서 최대 1.81·1.71배였다.
/// 현재 부하 분류 문턱은 3배다. 클럭 상태에 따라 1배 미만도 나올 수 있으며,
/// 이 값들이 다른 환경의 유휴 변동 상한을 보장하지는 않는다.
pub static CPU_FAMILY: Family = Family {
    kind: "고정 CPU 일감",
    quiet_below: 2.0,
    starved_at: Some(3.0),
    idle_worst: 1.81,
};

/// 실제 지연 시험에는 사용하지 않는다. 유휴 뒤에도 3.73·4.81배가 관측돼
/// 외부 부하와 구분되지 않았다. 부하 조건에서 변동 범위와 판정 문턱을 정하기 전에는 적용하지 않는다.
pub static SPAWN_FAMILY: Family = Family {
    kind: "자식 하나 띄우기",
    quiet_below: 2.0,
    starved_at: None,
    idle_worst: 4.81,
};

/// 유휴 뒤 왕복이 32~37배까지 증가한 측정이 있어 실제 지연 시험에는 사용하지 않는다.
pub static CHANNEL_FAMILY: Family = Family {
    kind: "같은 상대편으로의 값싼 왕복",
    quiet_below: 2.0,
    starved_at: None,
    idle_worst: 37.0,
};

/// 대조군 표본의 분류.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Band {
    /// 이 대조군의 증가가 확인되지 않았다.
    Quiet,
    /// 유휴 변동과 부하 증가를 구분할 수 없다.
    Undecidable,
    /// 이 계열의 부하 분류 문턱에 도달했다.
    Starved,
}

/// CPU 대조군의 일감 크기. 50_000은 CPU 포화에 반응하지 않아 2_000_000을 사용한다.
/// 값을 줄이기 전에는 조용한 상태에서 보정한 뒤 코어 수의 10배 busy 스레드 아래서
/// 3회 연속 비율이 2.0을 넘는지 확인한다. 500_000~750_000에서는 부하 중 보정에도
/// 큰 변동이 관측돼 그 범위에서 떨어진 값을 선택했다.
///
/// 이 부하 시험은 다른 시간 제한 시험을 방해하므로 일반 자동 검사에 넣지 않는다.
/// 이 크레이트의 통과만으로 실제 CPU 포화에 대한 반응까지 검증됐다고 볼 수 없다.
const SPIN_ITERS: u64 = 2_000_000;

/// 기준선은 반복 측정의 최솟값을 쓴다. 평균보다 선점의 영향을 덜 받지만
/// 기준선에 부하가 반영될 가능성 자체를 없애지는 않는다.
const CALIBRATION_ROUNDS: usize = 9;

/// spawn 보정에서 생성할 자식 수. 횟수를 늘려도 유휴 변동이 수렴하지 않아 3으로 둔다.
const SPAWN_CALIBRATION_ROUNDS: usize = 3;

/// 비용이 작은 왕복의 기준선 측정 횟수.
const ROUND_TRIP_CALIBRATION_ROUNDS: usize = 9;

/// CPU 표본은 1회 측정한다. 현재 일감의 유휴 20코어 측정에서는 최대 1.00배였지만,
/// 많은 시험이 병렬로 도는 조건의 best-of-1과 best-of-3 비교는 아직 없다.
/// spawn·왕복 표본은 긴 지연의 영향을 줄이려고 3회 중 최솟값을 사용한다.
const CPU_SAMPLE_ROUNDS: usize = 1;
const SPAWN_SAMPLE_ROUNDS: usize = 3;
const ROUND_TRIP_SAMPLE_ROUNDS: usize = 3;

/// 자식 프로세스를 실행하고 종료를 기다린다. 실행 실패는 Duration::ZERO다.
/// 기준선이 0이면 ratio는 1.0을 반환해 근거 없는 부하 판정을 피한다.
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

/// 측정 전에 같은 상대편으로의 값싼 왕복을 보정한다.
/// trip은 측정 대상 코드를 지나지 않아야 하며 호출자가 제공한다.
pub fn calibrate_round_trip(mut trip: impl FnMut() -> Duration) -> Duration {
    let mut baseline = trip();
    for _ in 1..ROUND_TRIP_CALIBRATION_ROUNDS {
        baseline = baseline.min(trip());
    }
    baseline
}

/// 채널 왕복 표본 하나. 꼬리가 커서 최소값으로 잡는다(`ROUND_TRIP_SAMPLE_ROUNDS`).
pub fn round_trip_sample(mut trip: impl FnMut() -> Duration, baseline: Duration) -> ControlSample {
    let mut cost = trip();
    for _ in 1..ROUND_TRIP_SAMPLE_ROUNDS {
        cost = cost.min(trip());
    }
    ControlSample::from_parts_in(&CHANNEL_FAMILY, cost, baseline)
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

/// 고정 CPU 일감의 현재 비용을 잰다. 디스크·IPC 상대편 대기의 대조군으로 대신 쓸 수 없다.
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
            family: &CPU_FAMILY,
        }
    }
}

/// 대조군 한 번의 결과.
#[derive(Debug, Clone, Copy)]
pub struct ControlSample {
    cost: Duration,
    baseline: Duration,
    /// 사용한 대조군 종류. 진단에 함께 표시한다.
    family: &'static Family,
}

impl ControlSample {
    /// 잰 값으로 만든다 — 산술을 부하 없이 고정하는 시험용.
    pub fn from_parts(cost: Duration, baseline: Duration) -> Self {
        Self::from_parts_in(&CPU_FAMILY, cost, baseline)
    }

    pub fn from_parts_in(family: &'static Family, cost: Duration, baseline: Duration) -> Self {
        Self {
            cost,
            baseline,
            family,
        }
    }

    pub fn kind(&self) -> &'static str {
        self.family.kind
    }

    /// 이 표본이 어느 구간에 있나 — 세 구간의 정의는 [`Family`].
    pub fn band(&self) -> Band {
        let r = self.ratio();
        if r < self.family.quiet_below {
            return Band::Quiet;
        }
        match self.family.starved_at {
            Some(t) if r >= t => Band::Starved,
            _ => Band::Undecidable,
        }
    }

    pub fn cost(&self) -> Duration {
        self.cost
    }

    pub fn baseline(&self) -> Duration {
        self.baseline
    }

    /// 기준선 대비 비율. 기준선이 0이면 비교 근거가 없어 1.0을 반환한다.
    pub fn ratio(&self) -> f64 {
        let base = self.baseline.as_secs_f64();
        if base <= 0.0 {
            return 1.0;
        }
        self.cost.as_secs_f64() / base
    }

    /// 정상 구간을 벗어났는지 확인한다. Undecidable도 포함하므로 부하가 확인됐다는 뜻은 아니다.
    pub fn is_inflated(&self) -> bool {
        self.band() != Band::Quiet
    }
}

/// 변이 크기와 대조군 잡음을 구분하는 문턱. Family의 부하 분류 문턱과 용도가 다르다.
/// 관측된 유휴 변동 1.81~2.4배보다 높고 인위적 지연의 약 500배보다 낮게 잡았다.
/// 이 측정은 다른 환경에서도 같은 잡음 상한이 유지된다는 보장은 아니다.
pub const MUTATION_MARGIN: f64 = 10.0;

/// 인위적 지연을 넣은 뒤 대조군 비율이 변이 문턱 아래인지 확인한다.
/// 정상/부하 분류와는 별개의 검사다.
pub fn control_stayed_out_of_the_path(sample: &ControlSample) -> bool {
    sample.ratio() < MUTATION_MARGIN
}

/// 표본이 기준선과 다른지 확인한다. 기준선을 그대로 반환하는 결함을 검출하는
/// 보조 검사이며, 값이 다르다는 사실만으로 측정 경로 전체를 검증하지는 않는다.
pub fn control_was_actually_resampled(sample: &ControlSample) -> bool {
    sample.cost() != sample.baseline()
}

pub fn families_separated(moved: &ControlSample, held: &ControlSample) -> bool {
    moved.ratio() > held.ratio() * MUTATION_MARGIN
}

/// 측정값·상한·대조군 분류를 실패 진단으로 만든다.
pub fn latency_verdict(
    what: &str,
    measured: Duration,
    limit: Duration,
    sample: &ControlSample,
) -> String {
    let ratio = sample.ratio();
    let kind = sample.family.kind;
    match sample.band() {
        Band::Starved => format!(
            "{what}: {measured:?}, 상한 {limit:?} 초과. 대조군({kind})도 기준선의 {ratio:.1}배다({:?} → {:?}). 부하 증가로 분류됐으므로 이 실패만으로 코드 회귀를 단정할 수 없다. 측정 조건을 확인하기 전에 상한을 올리지 않는다.",
            sample.baseline, sample.cost,
        ),
        Band::Undecidable => format!(
            "{what}: {measured:?}, 상한 {limit:?} 초과. 대조군({kind})은 기준선의 {ratio:.1}배다({:?} → {:?}). 이 값으로는 원인을 구분할 수 없다. 유휴만으로 {:.1}배까지 관측된 계열이므로 조용한 조건에서 다시 측정한다.",
            sample.baseline, sample.cost, sample.family.idle_worst,
        ),
        Band::Quiet => format!(
            "{what}: {measured:?}, 상한 {limit:?} 초과. 대조군({kind})은 기준선의 {ratio:.1}배다({:?} → {:?}). 이 대조군의 증가는 확인되지 않았다. 다른 자원의 대기나 측정 대상의 지연 원인은 별도로 확인한다.",
            sample.baseline, sample.cost,
        ),
    }
}

/// 명시적 verdict 전에 다른 단정이 패닉해도 Drop에서 대조군 값을 남긴다.
/// 정상 종료 때는 출력하지 않는다.
pub struct ControlProbe {
    label: String,
    work: fn() -> Duration,
    family: &'static Family,
    baseline: Duration,
    sample_rounds: usize,
    reported: bool,
}

impl ControlProbe {
    /// CPU 대조군을 측정 대상 실행 전에 보정한다.
    pub fn start(label: impl Into<String>) -> Self {
        Self::with(
            label,
            spin,
            CALIBRATION_ROUNDS,
            CPU_SAMPLE_ROUNDS,
            &CPU_FAMILY,
        )
    }

    /// 자식 실행 대조군. fork/exec·적재 비용을 CPU 대조군으로 대신할 수 없다.
    pub fn start_spawn(label: impl Into<String>) -> Self {
        Self::with(
            label,
            spawn_probe,
            SPAWN_CALIBRATION_ROUNDS,
            SPAWN_SAMPLE_ROUNDS,
            &SPAWN_FAMILY,
        )
    }

    fn with(
        label: impl Into<String>,
        work: fn() -> Duration,
        rounds: usize,
        sample_rounds: usize,
        family: &'static Family,
    ) -> Self {
        Self {
            label: label.into(),
            work,
            family,
            baseline: calibrate_with(work, rounds),
            sample_rounds,
            reported: false,
        }
    }

    fn sample(&self) -> ControlSample {
        let mut cost = (self.work)();
        for _ in 1..self.sample_rounds {
            cost = cost.min((self.work)());
        }
        ControlSample {
            cost,
            baseline: self.baseline,
            family: self.family,
        }
    }

    /// 진단을 반환한다. 이후 Drop은 같은 결과를 중복 출력하지 않는다.
    pub fn verdict(&mut self, measured: Duration, limit: Duration) -> String {
        self.reported = true;
        let sample = self.sample();
        latency_verdict(&self.label, measured, limit, &sample)
    }
}

/// 아직 진단을 반환하지 않은 상태에서 패닉 중일 때만 Drop 출력을 허용한다.
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
            "[대조군 {} · {}] 진단 반환 전 패닉이 발생했다. 대조군은 기준선의 {:.1}배다({:?} → {:?}). 이 값만으로 패닉 원인을 단정하지 않는다.",
            self.label,
            self.family.kind,
            sample.ratio(),
            sample.baseline(),
            sample.cost(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 통과한 시험의 측정값도 기록한다. libtest에서는 --show-output 또는 --nocapture로 확인한다.
    /// 시험 이름은 함수 경로에서 얻고 [latency-control] 접두사는 로그 수집에 사용한다.
    macro_rules! observe {
        ($($k:ident = $v:expr),+ $(,)?) => {{
            fn here() {}
            let path = std::any::type_name_of_val(&here);
            let name = path
                .strip_suffix("::here")
                .unwrap_or(path)
                .rsplit("::")
                .next()
                .unwrap_or(path);
            let mut line = format!("[latency-control] {name}");
            $( line.push_str(&format!(" {}={}", stringify!($k), $v)); )+
            println!("{line}");
        }};
    }

    /// 측정값의 소수점 자릿수를 통일한다.
    fn n(v: f64) -> String {
        format!("{v:.2}")
    }

    /// 인위적 지연은 현재 기준선의 배수로 정한다. 고정 시간은 느린 러너에서
    /// 기준선 대비 비율이 낮아져 변이를 놓칠 수 있다. 문턱의 두 배를 사용해
    /// CPU 일감 비용이 기준선보다 작아져도 검출 여유를 둔다.
    const INJECTED_OVER_BASELINE: f64 = 2.0 * MUTATION_MARGIN;

    /// 측정 대상 경로에 기준선 비례 지연을 넣어 대조군의 독립성을 확인한다.
    /// 앞뒤 표본의 비율만 비교하면 둘 다 같은 경로를 지날 때 지연이 상쇄되므로
    /// 보정한 기준선과 비교한다.
    #[test]
    fn an_artificial_delay_in_the_measured_path_does_not_move_the_control() {
        let control = CpuControl::calibrate();
        let injected = control.baseline().mul_f64(INJECTED_OVER_BASELINE);
        let before = control.sample();
        // sleep의 유휴 반응과 섞이지 않도록 바쁜 시간으로 지연을 넣는다.
        busy_for(injected);
        let after = control.sample();

        // 같은 판정 함수가 측정 경로를 지난 합성 표본을 거절하는지도 확인한다.
        let as_if_it_traversed = ControlSample::from_parts_in(
            &CPU_FAMILY,
            after.baseline() + injected,
            after.baseline(),
        );

        // baseline_ms와 injected_ms는 이번 측정값이고 injected_ratio는 구성상 상수다.
        observe!(
            before = n(before.ratio()),
            after = n(after.ratio()),
            margin = n(MUTATION_MARGIN),
            baseline_ms = n(control.baseline().as_secs_f64() * 1e3),
            injected_ms = n(injected.as_secs_f64() * 1e3),
            injected_ratio = n(as_if_it_traversed.ratio()),
        );
        assert!(
            control_stayed_out_of_the_path(&after),
            "{}",
            control_independence_verdict(&before, &after, injected)
        );
        assert!(
            !control_stayed_out_of_the_path(&as_if_it_traversed),
            "control_stayed_out_of_the_path가 측정 경로를 지난 합성 표본({:.1}배 = 기준선 {:?} + 주입 {:?})을 허용했다. 주입은 기준선의 {INJECTED_OVER_BASELINE}배다. MUTATION_MARGIN({MUTATION_MARGIN})과 판정 함수를 확인한다.",
            as_if_it_traversed.ratio(),
            after.baseline(),
            injected,
        );
    }

    /// 보정 직후와 지연 주입 뒤 표본을 구분해 기록한다. 지속 부하에서도 두 경우가
    /// 관측될 수 있으므로 어느 시점에 증가했는지만으로 원인을 확정하지 않는다.
    fn control_independence_verdict(
        before: &ControlSample,
        after: &ControlSample,
        injected: Duration,
    ) -> String {
        if before.is_inflated() {
            return format!(
                "보정 직후부터 대조군이 기준선의 {:.1}배다({:?} → {:?}). 지연 주입 전 증가이므로 이 결과로 코드 경로의 독립성을 판단할 수 없다. 부하와 보정·표본의 작업 차이를 확인한다.",
                before.ratio(),
                before.baseline(),
                before.cost(),
            );
        }
        format!(
            "지연 주입 뒤 대조군이 증가했다: 주입 전 {:.1}배, 주입 {injected:?}, 주입 뒤 {:.1}배({:?}). 측정 대상 경로의 영향과 그 시간의 부하는 이 측정만으로 구분할 수 없다. 조용한 조건에서 다시 확인한다.",
            before.ratio(),
            after.ratio(),
            after.cost(),
        )
    }

    /// 두 시점의 진단이 서로 구분되는지 확인한다.
    #[test]
    fn the_independence_verdict_separates_a_broken_control_from_a_reactive_one() {
        let base = Duration::from_micros(170);

        let injected = base * 20;
        let broken = control_independence_verdict(
            &ControlSample::from_parts(base * 900, base),
            &ControlSample::from_parts(base * 900, base),
            injected,
        );
        assert!(broken.contains("보정 직후부터"), "{broken}");
        assert!(!broken.contains("규칙 2\n"), "{broken}");
        assert!(
            !broken.contains("지연 주입 뒤 대조군이 증가했다"),
            "{broken}"
        );

        let reactive = control_independence_verdict(
            &ControlSample::from_parts(base, base),
            &ControlSample::from_parts(base * 900, base),
            injected,
        );
        assert!(
            reactive.contains("지연 주입 뒤 대조군이 증가했다"),
            "{reactive}"
        );
        assert!(
            reactive.contains("이 측정만으로 구분할 수 없다"),
            "{reactive}"
        );
        assert!(!reactive.contains("보정 직후부터"), "{reactive}");
    }

    // spawn의 독립성 변이 시험은 없다. sleep은 유휴 변동을, busy 루프는 CPU 경합을
    // 만들어 측정 대상 코드의 영향과 섞인다. 확인 방법을 정하기 전에는 실제 시험에 적용하지 않는다.

    // 아래 왕복은 API 시험용이며 실제 IPC 대조군이 아니다. 실제 사용에서는
    // 측정 대상과 같은 상대편을 지나야 디스크 대기 등의 영향을 비교할 수 있다.

    /// 답만 돌려주는 상대편 스레드. `delay` 를 올리면 상대편이 막힌 것을 흉내낸다.
    struct Peer {
        tx: std::sync::mpsc::Sender<u64>,
        rx: std::sync::mpsc::Receiver<u64>,
        delay: std::sync::Arc<std::sync::atomic::AtomicU64>,
    }

    fn spawn_peer() -> Peer {
        let (tx, in_rx) = std::sync::mpsc::channel::<u64>();
        let (out_tx, rx) = std::sync::mpsc::channel::<u64>();
        let delay = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
        let peer_delay = delay.clone();
        std::thread::spawn(move || {
            while let Ok(v) = in_rx.recv() {
                let nanos = peer_delay.load(std::sync::atomic::Ordering::Relaxed);
                if nanos > 0 {
                    std::thread::sleep(Duration::from_nanos(nanos));
                }
                if out_tx.send(v).is_err() {
                    break;
                }
            }
        });
        Peer { tx, rx, delay }
    }

    /// sleep하지 않고 지정 시간 동안 계산한다.
    fn busy_for(how_long: Duration) {
        let started = Instant::now();
        while started.elapsed() < how_long {
            spin();
        }
    }

    fn trip(peer: &Peer) -> Duration {
        let started = Instant::now();
        peer.tx.send(1).expect("상대편 살아 있음");
        peer.rx.recv().expect("상대편 응답");
        started.elapsed()
    }

    // 왕복의 코드 경로 독립성도 아직 검증하지 못했다. sleep과 busy 모두 왕복 비용에
    // 영향을 주므로 아래 상대편 지연 반응만으로 실제 지연 시험에 적용하지 않는다.

    /// 상대편에 지연을 넣으면 왕복 대조군이 반응하는지 확인한다.
    #[test]
    fn the_round_trip_control_moves_when_the_peer_is_blocked() {
        let peer = spawn_peer();
        let baseline = calibrate_round_trip(|| trip(&peer));
        // 여기서는 상대편 지연에 대한 반응만 검사하고 유휴 잡음 상한은 단정하지 않는다.
        peer.delay.store(
            Duration::from_millis(5).as_nanos() as u64,
            std::sync::atomic::Ordering::Relaxed,
        );
        let blocked = round_trip_sample(|| trip(&peer), baseline);
        observe!(
            ratio = n(blocked.ratio()),
            band = format!("{:?}", blocked.band()),
        );
        assert!(
            blocked.is_inflated(),
            "상대편에 5 ms 지연을 넣었지만 대조군 비율이 {:.1}배로 증가 판정에 도달하지 않았다({:?} → {:?})",
            blocked.ratio(),
            blocked.baseline(),
            blocked.cost(),
        );
        assert!(blocked.kind() == CHANNEL_FAMILY.kind);

        // 기준선과 같은 합성 표본은 증가로 판정하지 않아야 한다.
        let quiet =
            ControlSample::from_parts_in(&CHANNEL_FAMILY, blocked.baseline(), blocked.baseline());
        assert!(
            !quiet.is_inflated(),
            "is_inflated가 기준선과 같은 합성 표본({:.2}배)을 증가로 판정했다",
            quiet.ratio()
        );
    }

    /// 상대편 지연에 대한 왕복·CPU 대조군의 반응 차이를 확인한다.
    /// 변동 범위가 정해지지 않은 spawn 계열은 이 검사에서 제외한다.
    #[test]
    fn blocking_only_the_peer_moves_only_the_channel_family() {
        let peer = spawn_peer();
        let trip_baseline = calibrate_round_trip(|| trip(&peer));
        let cpu = CpuControl::calibrate();

        peer.delay.store(
            Duration::from_millis(5).as_nanos() as u64,
            std::sync::atomic::Ordering::Relaxed,
        );

        let channel = round_trip_sample(|| trip(&peer), trip_baseline);
        let scheduler = cpu.sample();

        observe!(
            channel = n(channel.ratio()),
            scheduler = n(scheduler.ratio()),
            separation = n(channel.ratio() / scheduler.ratio().max(f64::MIN_POSITIVE)),
            margin = n(MUTATION_MARGIN),
        );
        assert!(
            channel.is_inflated(),
            "상대편을 막았는데 채널 계열이 {:.1} 배에 그쳤다",
            channel.ratio()
        );
        // 기준선을 그대로 반환하는 결함을 먼저 확인한다.
        assert!(
            control_was_actually_resampled(&scheduler),
            "CPU 표본이 기준선과 같다({:?}). 이 표본으로 재측정 여부를 확인할 수 없다",
            scheduler.cost()
        );
        // 같은 판정 함수가 기준선과 같은 합성 표본을 거절해야 한다.
        let never_resampled =
            ControlSample::from_parts_in(&CPU_FAMILY, scheduler.baseline(), scheduler.baseline());
        assert!(
            !control_was_actually_resampled(&never_resampled),
            "control_was_actually_resampled가 기준선과 같은 합성 표본을 허용했다"
        );
        // CPU 비용이 전혀 변하지 않는지 대신 두 계열의 증가 비율 차이를 비교한다.
        assert!(
            families_separated(&channel, &scheduler),
            "왕복 {:.0}배, CPU {:.1}배: 비율 차이 {:.1}배가 문턱 {:.0}배에 도달하지 않았다",
            channel.ratio(),
            scheduler.ratio(),
            channel.ratio() / scheduler.ratio().max(f64::MIN_POSITIVE),
            MUTATION_MARGIN
        );

        // 같은 비율로 증가한 합성 표본은 독립으로 판정하지 않아야 한다.
        let as_if_one_family = ControlSample::from_parts_in(
            &CPU_FAMILY,
            scheduler.baseline().mul_f64(channel.ratio()),
            scheduler.baseline(),
        );
        assert!(
            !families_separated(&channel, &as_if_one_family),
            "families_separated가 왕복과 같은 비율({:.0}배)의 합성 CPU 표본을 독립으로 판정했다",
            as_if_one_family.ratio()
        );

        // CPU 포화에 대한 반응은 이 검사로 확인하지 못한다. 별도의 부하 측정이 필요하다.
    }

    /// 실제 부하 없이 각 분류의 진단을 확인한다.
    #[test]
    fn the_verdict_separates_a_starved_runner_from_a_slow_code_path() {
        let base = Duration::from_micros(170);
        let limit = Duration::from_millis(50);
        let measured = Duration::from_millis(80);

        let starved = ControlSample::from_parts(base * 9, base);
        let msg = latency_verdict("await 즉시 반환", measured, limit, &starved);
        assert!(msg.contains("부하 증가로 분류"), "{msg}");
        assert!(msg.contains("코드 회귀를 단정할 수 없다"), "{msg}");
        assert!(msg.contains("상한을 올리지 않는다"), "{msg}");

        let quiet = ControlSample::from_parts(base * 3 / 2, base);
        let msg = latency_verdict("await 즉시 반환", measured, limit, &quiet);
        assert!(msg.contains("이 대조군의 증가는 확인되지 않았다"), "{msg}");
        assert!(!msg.contains("부하 증가로 분류"), "{msg}");

        let murky = ControlSample::from_parts(base * 5 / 2, base);
        let msg = latency_verdict("await 즉시 반환", measured, limit, &murky);
        assert!(msg.contains("이 값으로는 원인을 구분할 수 없다"), "{msg}");
        assert!(msg.contains("유휴만으로"), "{msg}");
        assert!(!msg.contains("이 대조군의 증가는 확인되지 않았다"), "{msg}");
        assert!(!msg.contains("부하 증가로 분류"), "{msg}");

        // spawn 계열은 아무리 커도 굶주림을 주장하지 않는다.
        let big_spawn = ControlSample::from_parts_in(&SPAWN_FAMILY, base * 50, base);
        let msg = latency_verdict("무엇", measured, limit, &big_spawn);
        assert!(msg.contains("이 값으로는 원인을 구분할 수 없다"), "{msg}");
        assert!(!msg.contains("부하 증가로 분류"), "{msg}");
        assert!(msg.contains(SPAWN_FAMILY.kind), "{msg}");
        assert!(
            latency_verdict("무엇", measured, limit, &quiet).contains(CPU_FAMILY.kind),
            "CPU 계열 문장이 계열을 안 밝힌다"
        );
        let spawn_msg = latency_verdict(
            "무엇",
            measured,
            limit,
            &ControlSample::from_parts_in(&SPAWN_FAMILY, base * 2, base),
        );
        assert!(spawn_msg.contains(SPAWN_FAMILY.kind), "{spawn_msg}");
        assert!(!spawn_msg.contains(CPU_FAMILY.kind), "{spawn_msg}");
    }

    /// 분류 경계의 양쪽을 확인한다.
    #[test]
    fn the_inflation_threshold_is_a_boundary_not_a_slope() {
        let base = Duration::from_micros(1000);
        assert_eq!(
            ControlSample::from_parts(base * 2 - Duration::from_micros(1), base).band(),
            Band::Quiet
        );
        assert_eq!(
            ControlSample::from_parts(base * 2, base).band(),
            Band::Undecidable
        );
        assert_eq!(
            ControlSample::from_parts(base * 3, base).band(),
            Band::Starved
        );

        // spawn은 starved_at이 없어 부하 증가로 분류하지 않는다.
        let far = ControlSample::from_parts_in(&SPAWN_FAMILY, base * 50, base);
        assert_eq!(
            far.band(),
            Band::Undecidable,
            "spawn 계열이 굶주림을 주장했다"
        );
        assert_eq!(
            ControlSample::from_parts_in(&CHANNEL_FAMILY, base * 50, base).band(),
            Band::Undecidable
        );
    }

    /// 기준선이 0이면 부하 증가를 주장하지 않는다.
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
        assert!(msg.contains("이 대조군의 증가는 확인되지 않았다"), "{msg}");
    }

    /// 패닉 여부와 진단 반환 여부의 네 조합을 확인한다.
    #[test]
    fn the_probe_speaks_only_on_the_panicking_path() {
        assert!(drop_should_speak(false, true));
        assert!(!drop_should_speak(true, true));
        assert!(!drop_should_speak(false, false));
        assert!(!drop_should_speak(true, false));
    }

    /// 진단을 반환한 뒤에는 Drop에서 중복 출력하지 않는다.
    #[test]
    fn taking_the_verdict_silences_the_drop() {
        let mut probe = ControlProbe::start("무엇");
        let _ = probe.verdict(Duration::from_millis(80), Duration::from_millis(50));
        assert!(!drop_should_speak(probe.reported, true));
    }
}
