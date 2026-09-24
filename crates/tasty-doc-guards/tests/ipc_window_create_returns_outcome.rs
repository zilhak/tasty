//! window.create·view.create가 요청을 예약했다는 응답으로 끝나지 않고 생성 결과를 반환하는 구조인지 확인한다(ADR-0007).
//! GUI·이벤트 루프는 실행하지 않는다. 완료 채널의 동작은 app/event.rs의 단위 테스트가,
//! 이 검사는 생성 경로의 Result·IpcCompletion·응답 함수 사용을 확인한다.

use std::path::PathBuf;

fn read(rel: &str) -> String {
    let p: PathBuf = tasty_doc_guards::repo_root().join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
}

/// window.create 조건부터 IpcStep::Handled 반환 전까지의 텍스트를 읽는다.
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
    let sig_end = src[sig_start..]
        .find('{')
        .map(|i| sig_start + i)
        .expect("create_new_window body brace not found");
    let sig = &src[sig_start..sig_end];
    assert!(
        sig.contains("-> Result<"),
        "create_new_window 은 생성 성공/실패를 요청자에게 돌려줄 수 있게 Result 를 반환해야 \
         한다 (ADR-0007). 지금 시그니처:\n{sig}"
    );
}

/// 첫 // 뒤를 제거한다. 문자열 내부의 //를 구별하는 Rust 렉서는 아니다.
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

    assert!(
        !code.contains(r#""scheduled""#),
        "window.create 핸들러에 scheduled 응답이 있다. 완료 채널로 실제 생성 결과를 반환해야 한다(ADR-0007).\n블록:\n{block}"
    );
    assert!(
        block.contains("IpcCompletion::new("),
        "window.create 핸들러가 완료 채널(IpcCompletion)을 만들지 않는다 (ADR-0007).\n블록:\n{block}"
    );
    assert!(
        block.contains("AppEvent::CreateWindow(") && block.contains("Some(completion)"),
        "window.create 는 완료 채널을 실은 AppEvent::CreateWindow(.., Some(completion)) 를 \
         보내야 한다 (ADR-0007).\n블록:\n{block}"
    );
}

/// winit 이벤트 처리에서 완료 응답 함수를 사용하는지 확인한다. 실제 결과 전달 동작은 해당 함수의 단위 테스트가 맡는다.
#[test]
fn winit_handler_routes_the_outcome_through_the_contract_mapping() {
    let src = read("src/app/event_handler.rs");
    let start = src
        .find("AppEvent::CreateWindow(origin, completion) =>")
        .expect("CreateWindow winit handler arm not found");
    let tail = &src[start..];
    let arm = &tail[..tail.find("\n            }").unwrap_or(tail.len().min(600))];
    assert!(
        arm.contains("reply_window_create"),
        "CreateWindow 이벤트 처리에서 reply_window_create를 찾지 못했다. 생성 실패도 요청자에게 반환해야 한다(ADR-0007).\n처리 블록:\n{arm}"
    );
}
