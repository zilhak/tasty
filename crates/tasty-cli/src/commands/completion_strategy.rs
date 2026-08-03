//! `tasty completion-strategy` subcommand 정의 — 완료 판정 전략 레지스트리 조회.
//!
//! `hook-handler` CLI 구조를 미러링한다(원칙 2·3: id 로 직접 지정 — 다만 list 만
//! 있으므로 해당 없음, 전 범위 조회는 포커스 독립). reload/dispatch 대응물은
//! 없다 — 전략은 판정 함수이지 발화 대상이 아니고, user config 재로드는 아직
//! 노출하지 않는다(Settings UI CRUD 표면 없음).

use clap::Subcommand;

#[derive(Subcommand)]
pub enum CompletionStrategyCommands {
    /// List every registered completion strategy (host/plugin/user, incl. disabled).
    List,
}
