//! headless 메인 루프는 대기를 두 허브의 `min` 으로 계산한다 — 앱 허브(`app.timers`)와
//! plugin 매니저가 따로 소유한 허브다(`src/boot.rs` `run_headless`). 깨어난 바퀴가 그 두
//! 데드라인을 **다** 거둬야 대기가 다시 미래를 향한다.
//!
//! ## 무엇이 실제로 났나
//!
//! 한 바퀴의 시간축 함수(`run_due_timers`)가 앱 허브만 거두고 plugin 허브는
//! `Tick::Busy`(1 Hz) 편승과 PTY default wake 에서만 돌렸다. plugin 데드라인이 지나면
//! `recv_timeout(0)` → 할 일 없음 → 다시 대기를 다음 `Tick::Busy` 까지 되풀이했다 —
//! 실측으로 `PluginTick::Ping`(15 s) 마다 약 1 s 동안 루프 160 만 회였다. gui 는
//! `about_to_wait` 가 깨어날 때마다 `mgr.pump` 를 불러 이 형태가 없다.
//!
//! ## 왜 소스 가드인가
//!
//! 판정(`pump_plugins_if_due` 의 due 규칙)은 그 모듈의 단위 시험이 잰다. 여기서 묻는 것은
//! **그 함수가 루프의 시간축에서 불리는가** 다 — `App` 을 세우지 않고는 루프를 돌릴 수
//! 없어서, 호출 자리를 값으로 문다. 호출이 빠져도 컴파일과 두 조합의 유닛 스위트는
//! 초록이다(바쁜 대기는 틀린 답이 아니라 CPU 로만 보인다).

use super::callers_of;

/// 이 디렉토리의 가드는 판정 대상 밖이다 — 이 파일도 이름을 문자열로 든다.
const GUARD_DIRS: &[&str] = &["src/source_guards/", "crates/tasty-doc-guards/"];

/// plugin 허브를 거두는 함수가 한 바퀴의 시간축 함수 안에서 불리고, 그 시간축 함수는
/// `run_headless` 의 루프 안에서 불린다.
#[test]
fn the_headless_loop_turn_reaps_the_plugin_hub_too() {
    let reap = callers_of("pump_plugins_if_due", GUARD_DIRS);
    assert!(
        reap.iter()
            .any(|s| s.rel == "src/boot.rs" && s.caller.as_deref() == Some("run_due_timers")),
        "src/boot.rs 의 `run_due_timers` 가 `pump_plugins_if_due` 를 안 부른다 — plugin \
         데드라인이 지나면 headless 루프가 다음 Tick::Busy 까지 헛돈다. 찾은 호출: {:?}",
        reap.iter()
            .map(|s| (s.rel.as_str(), s.caller.as_deref(), s.line))
            .collect::<Vec<_>>()
    );
    let turn = callers_of("run_due_timers", GUARD_DIRS);
    assert!(
        turn.iter().any(|s| s.rel == "src/boot.rs"
            && s.caller.as_deref() == Some("run_headless")
            && s.in_loop),
        "`run_due_timers` 가 `run_headless` 의 루프 안에서 안 불린다 — 대조군이 죽었거나 \
         루프 구조가 바뀌었다. 찾은 호출: {:?}",
        turn.iter()
            .map(|s| (s.rel.as_str(), s.caller.as_deref(), s.in_loop))
            .collect::<Vec<_>>()
    );
}
