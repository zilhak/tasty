//! 완료 판정 전략 목록을 조회한다. 재로드·직접 실행 명령은 제공하지 않는다.

use clap::Subcommand;

#[derive(Subcommand)]
pub enum CompletionStrategyCommands {
    /// List every registered completion strategy (host/plugin/user, incl. disabled).
    List,
}
