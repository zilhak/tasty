//! `tasty hook-handler` subcommand 정의 — 공유 훅 핸들러 레지스트리 조회/재로드/발화.
//!
//! 훅 핸들러 조작은 에이전트 작업이라 CLI/IPC 양면 노출(원칙 2). 대상은 id 로 직접
//! 지정하고 list 는 전 범위(비활성 포함) 조회(원칙 3 포커스 독립). `file-handler`
//! CLI 구조를 미러링한다.

use clap::Subcommand;

#[derive(Subcommand)]
pub enum HookHandlerCommands {
    /// List every registered hook handler (host/plugin/user, incl. disabled).
    List,
    /// Reload `~/.tasty/hook-handlers.toml` (user source only; host/plugin unaffected).
    Reload,
    /// Manually fire a registered hook handler by id (test / automation entry point).
    ///
    /// For an IpcSequence handler, `--body`/`--header`/`--query` fill its
    /// `${body.x}`/`${header.x}`/`${query.x}` value slots. The response is an
    /// acknowledgement only — the handler runs fire-and-forget.
    Dispatch {
        /// Hook handler id to fire (e.g. `host/notify`, `user/wh-...`).
        #[arg(long)]
        id: String,
        /// Substitution body as a JSON value (object/array/scalar). Optional.
        #[arg(long)]
        body: Option<String>,
        /// Header substitution values as a JSON object `{"X-Sig":"abc"}`. Optional.
        #[arg(long)]
        header: Option<String>,
        /// Query substitution values as a JSON object `{"token":"t"}`. Optional.
        #[arg(long)]
        query: Option<String>,
    },
}
