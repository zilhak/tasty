//! CLI — `tasty-cli` 크레이트의 **boot 진입점만** 재수출한다.
//!
//! 본 바이너리는 CLI 파서를 소유하므로 boot 경로(`boot.rs` / `boot/cli_routing.rs`)가
//! 인자 파싱과 client 실행 진입점을 필요로 한다. 그 7개만 이름으로 노출한다.
//!
//! **glob(`pub use tasty_cli::*;`) 을 쓰지 않는 이유**: glob 이면 GUI 런타임·IPC
//! 핸들러·앱 상태가 `crate::cli::` 로 CLI 크레이트 전체에 닿을 수 있고, 그 계층
//! 위반이 컴파일 에러로 잡히지 않는다. 재사용이 필요한 코어는 CLI 가 아니라 양쪽이
//! 함께 쓰는 크레이트(`tasty-ssh` / `tasty-remote` / `tasty_ipc::client`)에 있다.
//! 같은 형태의 명시적 재수출: [`crate::ipc`].

pub use tasty_cli::{
    Cli, Commands, format_parse_error, localized_command, print_augmented_help, print_command_tree,
    run_client, try_run_plugin_cli,
};
