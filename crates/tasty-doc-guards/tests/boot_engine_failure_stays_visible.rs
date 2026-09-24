//! 엔진 생성에 실패해도 부팅 오류 화면이 유지되도록 소스의 호출·분기 형태를 검사한다.
//! 런처로 실행하면 stderr를 볼 수 없으므로 프로세스를 바로 종료하면 안 된다(ADR-0016).
//! GPU·ActiveEventLoop를 실행하지 않는 정적 검사이며, 화면 표시 자체를 검증하지는 않는다.
//! 오류별 진단 내용은 boot_machine.rs의 단위 테스트에서 확인한다.

use std::path::PathBuf;

fn read(rel: &str) -> String {
    let p: PathBuf = tasty_doc_guards::repo_root().join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
}

/// 함수 헤더 이후 첫 여는 중괄호부터 깊이가 0으로 돌아올 때까지 읽는다.
fn fn_body(src: &str, header: &str) -> String {
    let start = src
        .find(header)
        .unwrap_or_else(|| panic!("{header} not found"));
    let after = &src[start..];
    let open = after.find('{').expect("fn has no opening brace");
    let mut depth = 0i32;
    for (i, ch) in after[open..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return after[..open + i + 1].to_string();
                }
            }
            _ => {}
        }
    }
    panic!("{header} body has no matching close brace");
}

/// 각 줄의 첫 // 뒤를 제거한다. 문자열 내부 //도 구별하지 않는 간단한 판독이다.
fn code_only(s: &str) -> String {
    s.lines()
        .map(|l| match l.find("//") {
            Some(i) => &l[..i],
            None => l,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn boot_error_screen_renderer_exists() {
    let src = read("src/gfx/gpu/boot_error.rs");
    assert!(
        src.contains("fn render_boot_error"),
        "부팅 실패 화면 렌더러 render_boot_error가 없다(ADR-0016)."
    );
}

#[test]
fn engine_failure_routes_to_the_visible_error_path_not_a_blind_exit() {
    let src = read("src/app/boot_machine.rs");

    // 두 실패 갈래(워커 Err, disconnect fallback) 모두 진단을 보이는 경로로 넘긴다.
    let routed = src
        .matches("boot_error_info = Some(boot_engine_error_info(")
        .count();
    assert!(
        routed >= 2,
        "엔진 실패 두 경로가 모두 boot_error_info를 설정해야 한다(현재 {routed}건). 바로 종료하면 런처 사용자에게 오류가 보이지 않는다(ADR-0016)."
    );

    // 화면을 그리기 전에 종료하지 않도록 진단 빌더는 정보만 반환해야 한다.
    let builder = code_only(&fn_body(&src, "fn boot_engine_error_info("));
    assert!(
        !builder.contains("exit("),
        "boot_engine_error_info 가 프로세스를 종료한다 — 진단만 만들고 돌려줘야 화면을 \
         그린 뒤 사용자가 종료할 수 있다 (ADR-0016).\n{builder}"
    );
}

#[test]
fn event_loop_dispatches_and_renders_the_boot_error_screen() {
    let src = read("src/app/event_handler.rs");
    assert!(
        src.contains("self.boot_error_mode") && src.contains("handle_boot_error_window_event"),
        "window_event 가 boot_error_mode 를 분기해 handle_boot_error_window_event 로 \
         보내야 한다 (ADR-0016)."
    );
    assert!(
        src.contains("render_boot_error"),
        "boot error 이벤트 처리가 render_boot_error 로 화면을 그려야 한다 (ADR-0016)."
    );
}
