//! 훅이 남긴 **마커 파일**을 기다리는 한 자리.
//!
//! 사본이 셋이었다 — `hooks_detection_e2e` · `hook_env_integration` ·
//! `webhook_integration` 각자의 `wait_file_content`. 앞 둘은 바이트 동일이고 셋째만
//! `panic!` 대신 `assert!` 였다. 같은 물음에 답하는 자리가 셋이면 하나를 고쳐도 나머지
//! 둘은 안 고쳐진다. 통합 타깃끼리는 서로를 import 할 수 없어 `mod` 로 함께 쓴다.
//!
//! ## 만료가 두 가지를 뜻한다 — 그것을 가른다
//!
//! 종전 메시지는 `marker file … not written within 15s` 한 줄이었고, 그 한 줄이 **서로
//! 다른 두 사건**을 같은 말로 덮었다:
//!
//! 1. **훅이 안 돌았다** — 진짜 결함.
//! 2. **훅은 돌았는데 우리가 못 봤다** — 러너가 바빠 이 폴링 루프 자신이 굶었다.
//!    부하 의존 타임아웃(형태 C)이 이 얼굴을 하고 나타난다.
//!
//! 둘을 가르는 것은 **실제로 몇 번 확인했는가**다. 예산을 폴 간격으로 나눈 값이 기대치이고,
//! 그보다 훨씬 적으면 마커가 안 나온 것이 아니라 **우리가 안 본 것**이다 — 그 경우 상한을
//! 올리는 것은 처방이 아니다.
//!
//! 그래서 만료 메시지에 **경과 · 예산 · 확인 횟수와 기대치 · 부하**를 함께 낸다. 실측에서
//! 이 대기는 **한 번도 두 번째 확인을 넘기지 않았다** — 그러니 이 자리가 빨개졌다면 그것은
//! 평상시의 두 자릿수 배 밖 사건이고, 그때 필요한 것은 상한이 아니라 **무엇이 달랐는지** 다.

// 이유: 이 모듈을 include 하는 세 타깃이 각자 쓰는 함수가 달라, 안 쓰는 쪽에서 죽은 코드가 된다.
#![allow(dead_code)]

use std::path::Path;
use std::time::{Duration, Instant};

/// 폴 간격. 값 자체보다 **확인 횟수의 기준**이라는 점이 중요하다 — 만료 메시지가
/// `예산 / 이 값` 과 실제 횟수를 비교해 굶주림을 드러낸다.
pub const POLL: Duration = Duration::from_millis(50);

/// 마커 파일에 내용이 생길 때까지 기다려 trim 한 내용을 돌려준다.
pub fn wait_file_content(path: &Path, budget: Duration) -> String {
    wait_file_content_with_evidence(path, budget, || String::new())
}

/// 만료 시 호출자가 아는 것을 함께 싣는 형태. `evidence` 는 **만료 경로에서만** 불린다 —
/// 정상 경로에 비용을 얹지 않는다.
pub fn wait_file_content_with_evidence(
    path: &Path,
    budget: Duration,
    evidence: impl Fn() -> String,
) -> String {
    let start = Instant::now();
    let mut polls = 0usize;
    loop {
        polls += 1;
        if let Ok(content) = std::fs::read_to_string(path) {
            let trimmed = content.trim().to_string();
            if !trimmed.is_empty() {
                return trimmed;
            }
        }
        let elapsed = start.elapsed();
        if elapsed > budget {
            panic!(
                "{}",
                timeout_message(path, budget, elapsed, polls, &evidence())
            );
        }
        std::thread::sleep(POLL);
    }
}

/// 만료 진단문. **순수 함수다** — 파일도 시계도 안 건드리므로 아래 테스트가 문장을
/// 직접 찌를 수 있다.
fn timeout_message(
    path: &Path,
    budget: Duration,
    elapsed: Duration,
    polls: usize,
    evidence: &str,
) -> String {
    let expected = budget.as_secs_f64() / POLL.as_secs_f64();
    let mut msg = format!(
        "marker file {} not written within {budget:?}\n\
         경과 {elapsed:?} · 확인 {polls} 회 (예산/간격 = {expected:.0} 회 기대)",
        path.display()
    );
    if (polls as f64) < expected * 0.5 {
        msg.push_str(
            "\n★ 확인 횟수가 기대의 절반에 못 미친다 — 마커가 안 나온 것이 아니라 \
             **이 폴링 루프가 굶었다**. 상한을 올리는 것은 처방이 아니다.",
        );
    }
    if let Some(load) = load_average_1m() {
        msg.push_str(&format!("\n부하(1 분 평균) {load}"));
    }
    if !evidence.is_empty() {
        msg.push_str(&format!("\n호출자 증거: {evidence}"));
    }
    msg
}

/// 리눅스의 1 분 부하. 없으면 `None` — **부하가 원인이라는 판정에 쓰지 않는다.**
/// 실측에서 부하 평균은 지연을 예측하지 못했다(최대 지연이 load 19.6 에서 났고
/// load 30.3 회차는 평상값이었다). 진단문에 싣는 것은 그 회차가 어땠는지를 **기록**하려는
/// 것이지 원인으로 읽으라는 것이 아니다.
fn load_average_1m() -> Option<String> {
    let raw = std::fs::read_to_string("/proc/loadavg").ok()?;
    raw.split_whitespace().next().map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 굶주림 표지는 **확인 횟수가 적을 때만** 붙는다. 이게 없으면 진단문이 언제나
    /// 같은 말을 해서 두 사건을 다시 못 가른다.
    #[test]
    fn a_starved_loop_is_called_out_but_a_busy_one_is_not() {
        let p = Path::new("/tmp/nope");
        let starved = timeout_message(p, Duration::from_secs(15), Duration::from_secs(16), 3, "");
        assert!(starved.contains("굶었다"), "{starved}");

        let busy = timeout_message(p, Duration::from_secs(15), Duration::from_secs(16), 300, "");
        assert!(!busy.contains("굶었다"), "{busy}");
    }

    /// 진단문이 경과·예산·횟수를 **전부** 싣는다. 하나라도 빠지면 다음 사람이 지금의
    /// 우리와 같은 자리에서 계측을 다시 만들어야 한다.
    #[test]
    fn the_message_carries_what_the_next_reader_needs() {
        let msg = timeout_message(
            Path::new("/tmp/m"),
            Duration::from_secs(15),
            Duration::from_secs(15),
            300,
            "shell 출력에 트리거 텍스트 있음",
        );
        assert!(msg.contains("/tmp/m"));
        assert!(msg.contains("15s"));
        assert!(msg.contains("300 회"));
        assert!(msg.contains("shell 출력에 트리거 텍스트 있음"));
    }

    /// 호출자 증거가 없으면 빈 줄을 안 만든다.
    #[test]
    fn empty_evidence_adds_no_line() {
        let msg = timeout_message(
            Path::new("/tmp/m"),
            Duration::from_secs(1),
            Duration::from_secs(2),
            20,
            "",
        );
        assert!(!msg.contains("호출자 증거"), "{msg}");
    }
}
