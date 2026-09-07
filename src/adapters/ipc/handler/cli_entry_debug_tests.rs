//! CLI 진입점 중 **debug 빌드에만 있는** 것들.
//!
//! 형제 `cli_entry_tests` 와 재는 것은 같다 — clap 서브커맨드가 만든 `(메서드, params)`
//! 가 핸들러가 읽는 것과 맞는가. 파일을 가른 이유는 **배치 규율**이다: debug 로 게이트된
//! 항목은 `mod` 선언에 cfg 가 붙은 파일에 모은다
//! (`crates/tasty-doc-guards/tests/debug_handlers_live_in_cfg_declared_modules.rs` 가
//! `src/adapters/ipc/` 아래에서 이것을 강제한다). 형제 파일은 `#[cfg(test)]` 로만 선언돼
//! 있고 그 안의 시험 대부분이 release 에서도 유효하므로, 그 파일에 cfg 를 더 붙일 수 없다.

use tasty_cli::request::command_to_request;
use tasty_cli::{Commands, DebugCommands, ModalDebugCommands};

/// `tasty debug modal close-request` — 위 `window.close` 와 같은 이유로 핸들러를
/// 여기서 부를 수 없다(App 레벨이라 `self.view` 가 필요하다). 메서드 이름만 고정한다.
///
/// params 가 비어 있는 것이 이 명령의 계약이다 — 대상을 받지 않고 **활성 모달**에
/// 작용한다. 그래서 이름 하나가 틀리면 그대로 `Method not found` 가 되고, 그 실패는
/// "모달이 안 닫혔다" 와 같은 모양이 된다(실측으로 한 번 밟았다: 라우터에 등재하기
/// 전 호출이 `-32601` 을 냈는데 겉으로는 닫기가 안 먹은 것처럼 보였다).
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
