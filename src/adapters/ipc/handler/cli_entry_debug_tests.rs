//! 디버그 CLI가 만드는 요청의 메서드와 파라미터를 확인한다.
//! 디버그 전용 항목은 `mod` 선언에 cfg가 붙은 파일에 모아야 하므로
//! release에서도 실행하는 `cli_entry_tests`와 분리한다.

use tasty_cli::request::command_to_request;
use tasty_cli::{Commands, DebugCommands, ModalDebugCommands};

/// App 상태가 필요한 핸들러는 여기서 호출하지 않고 요청 형식을 확인한다.
/// 활성 모달을 닫는 명령이므로 대상 파라미터를 받지 않는다.
#[test]
fn modal_close_request_cli_entry_point_matches_the_registered_method() {
    let req = command_to_request(&Commands::Debug {
        command: DebugCommands::Modal(ModalDebugCommands::CloseRequest),
    });
    assert_eq!(req.method, "debug.modal.close_request");
    assert!(
        req.params.as_object().is_some_and(|o| o.is_empty()),
        "대상을 받지 않는 명령이다 — params 는 비어야 한다. 지금: {}",
        req.params
    );
}
