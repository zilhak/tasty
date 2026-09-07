//! plugin 종료의 **채널 순서 계약**을 못 박는다 — shutdown 요청은 `surface.closed`
//! **뒤에** 같은 `req_tx` 에 놓여야 한다.
//!
//! ## 계약
//!
//! plugin worker 는 자기 `req_tx` 를 순서대로 소비한다. 그래서 host 가 shutdown 요청을
//! surface 정리보다 **먼저** 넣으면, worker 는 자기 surface 들이 닫혔다는 것을 못 본 채
//! 종료 처리로 들어간다. 그 상태는 조용하다 — 컴파일도 되고, 종료는 어차피 프로세스를
//! 회수하므로 화면에도 로그에도 안 나온다. plugin 이 `surface.closed` 에서 하던 정리
//! (열린 파일 flush · 외부 프로세스 종료 · 캐시 저장)만 조용히 건너뛴다.
//!
//! ## 그 계약을 지키는 것은 **한 함수 안의 호출 배치**다
//!
//! `App::shutdown_step_closing_surfaces` 가 `shutdown_close_surfaces()` 를 부른 **뒤에**
//! `begin_plugin_shutdown()` 을 부른다. 요청 발송을 다음 스텝(S4 대기)으로 미루면 계약이
//! 깨지는 것이 아니라 — 그 스텝은 어차피 나중이라 순서는 유지된다 — **대기 겹침이
//! 사라진다.** 진짜로 깨지는 배치는 두 호출을 이 함수 안에서 맞바꾸는 것이다.
//!
//! ## 왜 이 가드가 필요했나
//!
//! 이 계약을 진술하는 주석이 **세 자리**에 흩어져 있었고 셋 다 산문뿐이었다:
//! `src/app/shutdown_machine.rs`(집행 자리) · `src/app/shutdown_cascade.rs`(요청 발송) ·
//! `crates/tasty-host-plugin/src/manager/lifecycle.rs`(수신 측). 사본이 셋이면 하나를
//! 옮길 때 나머지 둘이 낡는다. 지금은 집행 자리 하나가 계약을 들고 나머지 둘이 그리로
//! 가리키며, 값으로 무는 것은 이 가드다.
//!
//! ## 판정
//!
//! 지목한 함수 본문을 중괄호 균형으로 자르고, 주석을 지운 뒤 두 호출의 **위치**를
//! 비교한다. 함수를 못 자르면 통과가 아니라 실패다 — 이름이 바뀌었는데 조용히 초록이
//! 되는 것이 이 부류의 원래 사고다.

use super::{fn_body, repo_root, strip_comments};

/// 계약을 집행하는 자리.
const HOME: &str = "src/app/shutdown_machine.rs";
/// 그 안에서 배치가 결정되는 함수.
const STEP: &str = "fn shutdown_step_closing_surfaces";
/// 먼저 와야 하는 호출 — `surface.closed` 들을 `req_tx` 에 넣는다.
const FIRST: &str = "self.shutdown_close_surfaces()";
/// 뒤에 와야 하는 호출 — shutdown 요청을 같은 `req_tx` 에 넣는다.
const THEN: &str = "self.begin_plugin_shutdown()";

fn step_body() -> String {
    let path = repo_root().join(HOME);
    let src = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{HOME} 을 읽지 못했다: {e}"))
        .replace("\r\n", "\n");
    let body = fn_body(&src, STEP).unwrap_or_else(|| {
        panic!(
            "`{STEP}` 을 {HOME} 에서 자르지 못했다. 이름이 바뀌었거나 자리를 옮긴 것이니 \
             이 가드의 상수를 함께 고쳐라 — 못 자른 상태의 초록은 통과가 아니라 미측정이다"
        )
    });
    strip_comments(&body)
}

#[test]
fn the_shutdown_request_is_queued_after_the_surface_closes() {
    let body = step_body();
    let first = body.find(FIRST).unwrap_or_else(|| {
        panic!("`{STEP}` 안에 `{FIRST}` 가 없다 — surface 정리가 이 스텝에서 사라졌다")
    });
    let then = body.find(THEN).unwrap_or_else(|| {
        panic!(
            "`{STEP}` 안에 `{THEN}` 가 없다 — shutdown 요청 발송이 이 스텝 밖으로 나갔다. \
             나간 자리가 S4 대기 스텝이면 순서는 유지되지만 대기 겹침이 사라진다"
        )
    });
    assert!(
        first < then,
        "채널 순서 계약이 깨졌다: `{STEP}` 안에서 `{THEN}` 가 `{FIRST}` 보다 먼저 온다. \
         plugin worker 는 `req_tx` 를 순서대로 소비하므로, 그 배치에서는 worker 가 자기 \
         surface 가 닫혔다는 것을 못 본 채 종료 처리에 들어간다 — 컴파일도 되고 종료도 \
         되며 화면에도 로그에도 안 나온다. 계약의 자리는 {HOME} 이다"
    );
}

#[test]
fn the_guard_reads_a_body_that_actually_has_both_calls() {
    // 공허 방지 — 위 시험의 두 `find` 가 모두 실패해도 `unwrap_or_else` 가 패닉하므로
    // 죽기는 한다. 여기서 따로 못 박는 것은 **잘라 온 것이 그 함수인가**다: `fn_body`
    // 가 다른 함수를 잘라도 두 호출이 우연히 들어 있을 수 있다.
    let body = step_body();
    assert!(
        body.contains("self.set_shutdown_phase(ShutdownPhase::StoppingPlugins)"),
        "잘라 온 본문이 `{STEP}` 이 아니다 — 그 스텝의 상 전이가 안 보인다"
    );
}
