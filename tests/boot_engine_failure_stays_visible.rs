//! 부팅 엔진 생성 실패가 "창이 깜빡이고 사라지는 것" 으로 되돌아가지 않게 막는 가드.
//!
//! 배경: 부팅 중 엔진 생성 실패는 `tracing::error!` + `exit(1)` 이었다. 터미널에서
//! 실행한 사용자는 stderr 로 진단을 보지만, dock/시작 메뉴/런처로 실행한 사용자에게는
//! 창이 잠깐 떴다 사라지는 것이 전부였다. 이 단계는 부팅 GPU init 이후라 **GPU·창이
//! 살아있으므로**, `enter_shell_setup_mode` 선례대로 진단을 창에 그려 유지하도록 고쳤다
//! (`docs/adr/0117-window-and-modal-creation-failure-policy.md` 재검토 트리거 갱신).
//!
//! 이 경로는 winit `ActiveEventLoop` 와 GPU 가 있어야 돌아가 행동 테스트로 감쌀 수 없다
//! (ADR-0117 의 창 생성 경로와 같은 제약). 그래서 진단 소스는 단위 테스트
//! (`boot_machine.rs` 의 `boot_engine_error_info` — 세 키가 distinct)로, 실패가 **보이는
//! 채로 유지되는지** 는 이 소스 형태 가드로 고정한다. 선례:
//! `tests/no_panic_in_window_creation.rs`, `tests/ipc_window_create_returns_outcome.rs`.

use std::path::{Path, PathBuf};

fn read(rel: &str) -> String {
    let p: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
}

/// `fn <name>(` 부터 함수 본문 끝(같은 열의 닫는 중괄호)까지 대략 잘라낸다 —
/// 중괄호 깊이를 세어 0 으로 돌아오는 지점까지.
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

/// 줄 주석(`//`)을 제거해 설명 문구가 오탐되지 않게 한다.
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
        "부팅 실패 화면 렌더러 render_boot_error 가 없다 (ADR-0117 재검토 트리거)."
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
        "엔진 실패 두 갈래가 모두 boot_error_info 로 라우팅돼야 한다(현재 {routed}건) — \
         한쪽이라도 blind exit 로 되돌아가면 런처 사용자에게 안 보인다 (ADR-0117)."
    );

    // 진단 빌더 자체는 종료하지 않는다 — 화면을 그릴 수 있게 info 를 돌려줘야 한다.
    // exit 를 여기 넣으면 화면을 보이기 전에 프로세스가 죽어 결함이 재발한다.
    let builder = code_only(&fn_body(&src, "fn boot_engine_error_info("));
    assert!(
        !builder.contains("exit("),
        "boot_engine_error_info 가 프로세스를 종료한다 — 진단만 만들고 돌려줘야 화면을 \
         그린 뒤 사용자가 종료할 수 있다 (ADR-0117).\n{builder}"
    );
}

#[test]
fn event_loop_dispatches_and_renders_the_boot_error_screen() {
    let src = read("src/app/event_handler.rs");
    assert!(
        src.contains("self.boot_error_mode") && src.contains("handle_boot_error_window_event"),
        "window_event 가 boot_error_mode 를 분기해 handle_boot_error_window_event 로 \
         보내야 한다 (ADR-0117)."
    );
    assert!(
        src.contains("render_boot_error"),
        "boot error 이벤트 처리가 render_boot_error 로 화면을 그려야 한다 (ADR-0117)."
    );
}
