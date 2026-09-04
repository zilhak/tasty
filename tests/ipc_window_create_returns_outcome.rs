//! `window.create` / `view.create` 가 fire-and-forget 으로 되돌아가는 것을 막는 가드.
//!
//! 배경: 두 메서드는 `AppEvent::CreateWindow` 를 프록시에 밀어넣고 곧바로
//! `{"scheduled": true}` 를 돌려줬다. 창 생성이 실제로 성공했는지는 응답에 없었고,
//! 실패는 요청자(에이전트)에게 전혀 가지 않은 채 사용자 toast 로만 새어나갔다 —
//! 사용자가 요청하지도 않은 일의 실패 통지(원칙 1 위반). 완료 채널
//! (`IpcCompletion`)로 왕복시켜 성공/실패를 응답에 싣도록 고쳤다
//! (`docs/adr/0122-winit-scheduled-fallible-ipc-returns-outcome.md`).
//!
//! 그런데 "고쳤다" 와 "고쳐진 채로 유지된다" 는 다른 문제다. 이 경로는 winit
//! `ActiveEventLoop` 가 있어야 돌아가 행동 테스트로 감쌀 수 없다(`create_new_window`
//! 는 GPU·이벤트 루프 의존). 그래서 소스 형태를 고정한다 — `IpcCompletion` 단위
//! 테스트(`src/app/event.rs`)가 완료 채널의 *동작*을 잡고, 이 가드가 그 채널이 창
//! 생성 경로에 *배선된 채로 유지되는지* 를 잡는다.
//!
//! 선례: `tests/no_panic_in_window_creation.rs`(같은 이유로 소스 형태를 고정한다).

use std::path::{Path, PathBuf};

fn read(rel: &str) -> String {
    let p: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
}

/// `if cmd.request.method == "window.create" ...` 블록만 잘라낸다 —
/// `return IpcStep::Handled;` 까지.
fn window_create_block(src: &str) -> &str {
    let start = src
        .find(r#"cmd.request.method == "window.create""#)
        .expect("window.create handler block not found in app_methods.rs");
    let tail = &src[start..];
    let end = tail
        .find("return IpcStep::Handled;")
        .expect("window.create block has no terminating return");
    &tail[..end]
}

#[test]
fn create_new_window_returns_a_result() {
    let src = read("src/app/window_lifecycle.rs");
    let sig_start = src
        .find("fn create_new_window(")
        .expect("create_new_window not found");
    // 시그니처는 여는 중괄호 전까지 — 반환 타입이 그 안에 있다.
    let sig_end = src[sig_start..]
        .find('{')
        .map(|i| sig_start + i)
        .expect("create_new_window body brace not found");
    let sig = &src[sig_start..sig_end];
    assert!(
        sig.contains("-> Result<"),
        "create_new_window 은 생성 성공/실패를 요청자에게 돌려줄 수 있게 Result 를 반환해야 \
         한다 (ADR-0122). 지금 시그니처:\n{sig}"
    );
}

/// 줄에서 코드 부분만 남긴다 — 설명 주석(`//`)에 적힌 `"scheduled"` 언급이 오탐되지
/// 않게. 이 블록의 문자열 리터럴에는 `//` 가 없어 단순 절단으로 충분하다.
fn code_only(block: &str) -> String {
    block
        .lines()
        .map(|l| match l.find("//") {
            Some(i) => &l[..i],
            None => l,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn window_create_ipc_routes_through_a_completion_channel() {
    let src = read("src/app/ipc/app_methods.rs");
    let block = window_create_block(&src);
    let code = code_only(block);

    // fire-and-forget 의 흔적(즉시 scheduled 응답)이 이 블록의 코드에 다시 들어오면 fail.
    assert!(
        !code.contains(r#""scheduled""#),
        "window.create 핸들러가 fire-and-forget `{{\"scheduled\": true}}` 로 되돌아갔다 — \
         완료 채널로 실제 결과를 돌려줘야 한다 (ADR-0122).\n블록:\n{block}"
    );
    // 완료 채널이 실제로 배선돼 있어야 한다: CreateWindow 에 Some(completion) 을 싣고
    // IpcCompletion 을 만든다.
    assert!(
        block.contains("IpcCompletion::new("),
        "window.create 핸들러가 완료 채널(IpcCompletion)을 만들지 않는다 (ADR-0122).\n블록:\n{block}"
    );
    assert!(
        block.contains("AppEvent::CreateWindow(") && block.contains("Some(completion)"),
        "window.create 는 완료 채널을 실은 AppEvent::CreateWindow(.., Some(completion)) 를 \
         보내야 한다 (ADR-0122).\n블록:\n{block}"
    );
}
