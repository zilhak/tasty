//! 플러그인의 surface.closed 요청 뒤에 shutdown 요청을 같은 채널에 넣어야 한다.
//! 순서가 바뀌면 플러그인이 surface 종료에 필요한 정리를 하기 전에 종료할 수 있다.
//! shutdown_step_closing_surfaces의 두 호출 위치를 비교하며 실제 분기의 실행 여부는 확인하지 않는다.
//! shutdown 발송을 다음 대기 단계로 옮기면 순서가 유지돼도 다른 정리와 대기를 겹칠 수 없게 된다.

use super::{fn_body, repo_root, strip_comments};

const HOME: &str = "src/app/shutdown_machine.rs";
const STEP: &str = "fn shutdown_step_closing_surfaces";
const FIRST: &str = "self.shutdown_close_surfaces()";
const THEN: &str = "self.begin_plugin_shutdown()";

/// 첫 종료 프레임이 즉시 표시될 수 있으므로 네이티브 자식 숨김을 그보다 먼저 실행해야 한다.
#[test]
fn shutdown_entry_hides_native_children_before_the_first_drive() {
    let src = std::fs::read_to_string(repo_root().join(HOME)).unwrap();
    let body = strip_comments(&fn_body(&src, "fn begin_shutdown").unwrap());
    let hide = body.find("main.hide_webviews_for_shutdown()").unwrap();
    for drive in ["self.drive_shutdown_frame(", "self.run_shutdown_blocking("] {
        assert!(
            hide < body.find(drive).unwrap(),
            "hide must precede {drive}"
        );
    }
}

fn step_body() -> String {
    let path = repo_root().join(HOME);
    let src = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{HOME} 을 읽지 못했다: {e}"))
        .replace("\r\n", "\n");
    let body = fn_body(&src, STEP).unwrap_or_else(|| {
        panic!("{HOME}에서 `{STEP}` 본문을 읽지 못했다. 함수 이름·이동 여부와 추출을 확인한다.")
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
        "`{STEP}`에서 `{THEN}`이 `{FIRST}`보다 앞에 있다. surface 종료 처리가 shutdown 요청보다 먼저 전달되도록 {HOME}의 호출 순서를 고친다."
    );
}

#[test]
fn the_guard_reads_a_body_that_actually_has_both_calls() {
    // 함수 추출이 다른 본문을 반환하지 않았는지 상태 전환도 확인한다.
    let body = step_body();
    assert!(
        body.contains("self.set_shutdown_phase(ShutdownPhase::StoppingPlugins)"),
        "잘라 온 본문이 `{STEP}` 이 아니다 — 그 스텝의 상 전이가 안 보인다"
    );
}
