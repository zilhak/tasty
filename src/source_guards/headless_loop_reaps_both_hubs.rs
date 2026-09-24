//! 헤드리스 루프는 앱과 플러그인 타이머 중 이른 시각까지 대기하므로 두 타이머를 모두 처리해야 한다.
//! 만료된 플러그인 타이머를 남기면 대기 시간이 0이 되어 루프가 불필요하게 반복될 수 있다.
//! run_due_timers의 플러그인 처리 호출과 run_headless 루프의 run_due_timers 호출 위치를 확인한다.
//! 조건 분기의 실제 실행 여부나 타이머 처리 결과까지 검증하지는 않는다.

use super::callers_of;

const GUARD_DIRS: &[&str] = &["src/source_guards/", "crates/tasty-doc-guards/"];

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
        "run_headless 루프에서 run_due_timers 호출을 찾지 못했다. 루프 구조와 수집 범위를 확인한다: {:?}",
        turn.iter()
            .map(|s| (s.rel.as_str(), s.caller.as_deref(), s.in_loop))
            .collect::<Vec<_>>()
    );
}
