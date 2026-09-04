//! 인스턴스 spawn 의 **공통 설정과 실패 판정**을 두 하네스가 공유하는 자리.
//!
//! `tests/common`(범용 인스턴스)과 `tests/webhook_common`(웹훅 인스턴스)은 같은
//! `CARGO_BIN_EXE_tasty` 를 같은 방식으로 띄운다. 그런데 상한값이 서로 달랐고
//! (30/15 vs 40/20) 그 차이의 근거가 어디에도 없었다 — 같은 단계에 다른 잣대를
//! 대면 한쪽에서만 재현되는 flaky 가 생기고, 그때마다 "이 하네스는 원래 느린가" 를
//! 사람이 다시 판단해야 한다. 값을 여기 하나로 모은다. 자식의 로그 필터도 같다.
//!
//! 실패 원인 판정도 같이 둔다. spawn timeout 은 "느린 것" 과 "부팅이 아예 막힌 것"
//! 이 똑같은 메시지로 보였는데, 둘은 대응이 완전히 다르다(전자는 기다리면 되고
//! 후자는 기다려도 안 된다). 디스플레이 서버 부재는 stderr 시그니처로 갈리고,
//! 자식이 이미 죽은 경우는 조기 종료 감지로 갈린다.

#![allow(dead_code)] // 두 하네스가 각자 일부만 쓴다

use std::time::Duration;

/// 제품이 로그 필터로 읽는 환경변수. **`RUST_LOG` 이 아니다** —
/// `src/platform/crash_report.rs` 의 `EnvFilter::try_from_env("TASTY_LOG")` 다.
/// 한 번 `RUST_LOG` 로 잘못 넣어 두 하네스가 몇 달 동안 필터 없이 돌았으므로,
/// 이름은 상수로 고정하고 `tests/harness_log_env.rs` 가 제품 소스와 대조한다.
pub const LOG_ENV: &str = "TASTY_LOG";

/// 필터 문자열의 유일한 정의 자리. 두 상수가 같은 리터럴을 각자 적으면 한쪽만
/// 고쳐져 어긋나므로 매크로로 한 번만 쓴다(`concat!` 은 리터럴만 받는다).
macro_rules! product_default_filter {
    () => {
        "warn,wgpu_hal=error,wgpu_core=error,naga=error,egui_winit::clipboard=off"
    };
}
use product_default_filter;

/// 자식에게 주는 기본 로그 필터 — **제품 기본값과 같은 모양**이어야 한다.
///
/// `TASTY_LOG` 를 지정하는 순간 제품의 기본 필터는 통째로 대체된다. 그래서 `warn`
/// 한 단어만 주면 기본값에 들어 있던 억제(`wgpu_hal=error` 등)가 전부 풀려
/// **로그가 오히려 늘어난다** — 실측(격리 HOME, 정상 부팅, 12초): env 미지정 7줄 ·
/// `warn` 12줄(wgpu 5줄) · 이 값 7줄. 늘어난 줄은 `STDERR_TAIL_LINES`(30) 짜리
/// 진단 tail 을 그대로 밀어낸다. host 의 `TASTY_LOG=trace` 누수를 막으면서 노이즈는
/// 늘리지 않으려면 기본값과 같은 모양을 명시하는 수밖에 없다.
pub const LOG_FILTER: &str = product_default_filter!();

/// 웹훅 하네스용 필터. 리스너의 bind 성공/실패 판정에 그 타깃의 `info` 두 줄이
/// 필요하다 — 나머지는 [`LOG_FILTER`] 를 **그대로 앞에 두고** 뒤에만 덧붙인다.
pub const LOG_FILTER_WEBHOOK: &str =
    concat!(product_default_filter!(), ",tasty::webhook::listener=info");

/// S1 — `--port-file` 에 포트가 쓰이기까지. GUI 부팅(창 + GPU 디바이스 + boot
/// 상태기계)이 끝나야 IPC 가 시작되므로 이 단계가 가장 길다.
///
/// 값의 근거: dev cold path worst-case(GPU init + plugin discover/extract +
/// theme/db init, dev 프로필이 release 의 ~3.5 배) + self-hosted runner 변동 폭.
/// 두 하네스가 쓰던 30 s / 40 s 중 **큰 쪽**으로 맞춘다 — 웹훅 하네스의 상한을
/// 낮추는 것은 근거 없는 동작 축소이고, 반대로 올리는 쪽은 *이미 실패할 spawn* 이
/// 보고되기까지의 시간만 늘린다. 그 시간은 [`early_exit_message`] 경로가 대부분
/// 없앤다(자식이 죽었으면 상한을 기다리지 않는다).
pub const SPAWN_PORT_TIMEOUT: Duration = Duration::from_secs(40);

/// S2 — 첫 surface 의 PTY 가 프롬프트를 낼 때까지. S1 이 끝난 뒤라 GPU 와 무관하다.
pub const SPAWN_SHELL_TIMEOUT: Duration = Duration::from_secs(20);

/// stderr 시그니처로 가릴 수 있는 부팅 차단 원인.
enum BootBlocker {
    /// GPU/드라이버 스택을 못 잡았다 — 디바이스 경합이거나 미가용.
    Gpu(&'static str),
    /// 디스플레이 서버가 아예 없다 — winit 이 즉시 죽는다.
    NoDisplay(&'static str),
}

/// GPU/드라이버가 실패할 때 스택이 남기는 문자열들.
const GPU_MARKERS: &[&str] = &[
    "renderD128", // DRM 렌더 노드 — 다른 프로세스가 점유했거나 접근 불가
    "VK_ERROR_",  // Vulkan 초기화 실패 전반
    "DRI3",       // X 서버가 DRI3 를 못 주는 경우(가속 경로 상실)
    "libEGL",     // EGL 경고 — 위 둘과 함께 나오는 것이 보통
    "tu_knl",     // mesa/turnip 커널 인터페이스 오류
    "failed to open device",
];

/// 디스플레이 서버 부재를 가리키는 문자열들.
const NO_DISPLAY_MARKERS: &[&str] = &[
    "neither WAYLAND_DISPLAY nor WAYLAND_SOCKET nor DISPLAY is set",
    "cannot open display",
];

fn detect_blocker(stderr_tail: &str) -> Option<BootBlocker> {
    if let Some(m) = NO_DISPLAY_MARKERS
        .iter()
        .find(|m| stderr_tail.contains(**m))
    {
        return Some(BootBlocker::NoDisplay(m));
    }
    GPU_MARKERS
        .iter()
        .find(|m| stderr_tail.contains(**m))
        .map(|m| BootBlocker::Gpu(m))
}

/// stderr tail 에서 부팅 차단 원인을 찾아 판정문 한 줄을 만든다.
///
/// 이 줄이 없으면 "느린 건지 GPU 가 없는 건지" 를 매번 사람이 stderr 을 읽어
/// 판정해야 한다. 실제로 하루치 flaky 전건이 이 판정 하나로 갈렸다.
pub fn boot_blocker_verdict(stderr_tail: &str) -> Option<String> {
    match detect_blocker(stderr_tail)? {
        BootBlocker::Gpu(marker) => Some(format!(
            "GPU 경합/미가용으로 보인다 — 코드 인과가 아니다(시그니처: `{marker}`). GUI 부팅이 \
             GPU 디바이스를 못 잡으면 IPC 가 시작되지 않아 port file 이 끝내 안 써진다. 상한을 \
             늘려도 결과는 같다. 다른 워크트리·인스턴스가 같은 디바이스를 쓰고 있는지 먼저 본다."
        )),
        BootBlocker::NoDisplay(marker) => Some(format!(
            "디스플레이 서버가 없다 — 코드 인과가 아니다(시그니처: `{marker}`). 이 하네스는 실제 \
             GUI 를 띄우므로 `xvfb-run -a` 같은 디스플레이 위에서 돌려야 한다."
        )),
    }
}

fn verdict_or_default(tail: &str, fallback: &str) -> String {
    boot_blocker_verdict(tail).unwrap_or_else(|| fallback.to_string())
}

/// 자식이 이미 죽었을 때의 panic 메시지. 상한을 다 기다릴 이유가 없는 경우다 —
/// 부팅 실패는 대부분 즉사(디스플레이 없음·설정 오류)라, 이 경로가 실제 대기 시간을
/// 수십 초에서 1 초 미만으로 줄인다.
pub fn early_exit_message(status: &str, tail_lines: usize, tail: &str) -> String {
    let verdict = verdict_or_default(tail, "stderr 의 마지막 오류를 그대로 읽는다.");
    format!(
        "tasty 프로세스가 부팅 중 종료했다 ({status}) — 상한을 기다리지 않고 즉시 실패시킨다.\n{verdict}\n--- stderr (last {tail_lines} lines) ---\n{tail}"
    )
}

/// spawn timeout panic 메시지. 단계 이름·상한·판정·stderr tail 을 한 형식으로 묶어
/// 두 하네스가 같은 모양으로 실패하게 한다.
pub fn spawn_timeout_message(
    stage: &str,
    limit: Duration,
    tail_lines: usize,
    tail: &str,
) -> String {
    let verdict = verdict_or_default(
        tail,
        "부팅 차단 시그니처는 없다 — 부팅 지연이나 설정 경로를 본다.",
    );
    format!(
        "{stage} within {limit:?}.\n{verdict}\n--- stderr (last {tail_lines} lines) ---\n{tail}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-09-04 실측 로그. 워크트리 4 곳이 동시에 `cargo test` 를 돌려 GPU 디바이스가
    /// 경합했을 때 실제로 남은 stderr 이다 — 판정이 이 형태를 놓치면 의미가 없다.
    const REAL_GPU_CONTENTION_TAIL: &str = "\
libEGL warning: DRI3 error: Could not get DRI3 device
libEGL warning: Ensure your X server supports DRI3 to get accelerated rendering
TU: error: ../src/freedreno/vulkan/tu_knl.cc:387: failed to open device /dev/dri/renderD128 (VK_ERROR_INCOMPATIBLE_DRIVER)";

    #[test]
    fn real_gpu_contention_tail_is_called_out_as_environment_not_code() {
        let verdict = boot_blocker_verdict(REAL_GPU_CONTENTION_TAIL)
            .expect("실측 GPU 경합 로그는 판정되어야 한다");
        assert!(verdict.contains("코드 인과가 아니다"), "{verdict}");
        assert!(verdict.contains("GPU"), "{verdict}");
    }

    #[test]
    fn missing_display_is_reported_as_display_not_gpu() {
        let tail = "Error: os error at winit/src/platform_impl/linux/mod.rs:765: \
                    neither WAYLAND_DISPLAY nor WAYLAND_SOCKET nor DISPLAY is set.";
        let verdict = boot_blocker_verdict(tail).expect("디스플레이 부재도 판정 대상이다");
        assert!(verdict.contains("디스플레이 서버가 없다"), "{verdict}");
        assert!(
            verdict.contains("xvfb-run"),
            "다음 사람이 바로 조치할 수 있게 방법을 실어야 한다: {verdict}"
        );
    }

    #[test]
    fn an_ordinary_slow_boot_gets_no_false_verdict() {
        let tail = "INFO tasty: plugin discovery finished\nINFO tasty: theme loaded";
        assert!(boot_blocker_verdict(tail).is_none());
        let msg = spawn_timeout_message("tasty failed to start", SPAWN_PORT_TIMEOUT, 30, tail);
        assert!(msg.contains("부팅 차단 시그니처는 없다"), "{msg}");
    }

    #[test]
    fn both_harnesses_share_one_bound_for_the_same_stage() {
        // 값 자체보다 "두 하네스가 같은 상수를 본다" 는 사실이 중요하다. 이 모듈이
        // 유일한 정의 자리이므로, 어느 한쪽이 자기 값을 되살리면 여기 상수가 안 쓰여
        // dead_code 로 드러난다. 상한 순서(S1 > S2)만 여기서 고정한다.
        assert!(SPAWN_PORT_TIMEOUT > SPAWN_SHELL_TIMEOUT);
    }
}
