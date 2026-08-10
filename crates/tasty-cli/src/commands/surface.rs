//! `tasty surface` subcommand 정의 — surface 대상 액션(completion 등).

use clap::Subcommand;

#[derive(Subcommand)]
pub enum SurfaceCommands {
    /// Signal that a surface has completed its work (or needs input), raising
    /// attention highlight.
    ///
    /// Highlight 를 발동하는 producer 중 하나(release 정식). --surface 필수 —
    /// 포커스에 의존하지 않고 대상 surface 를 명시한다(불가침 원칙 1).
    Completion {
        /// Surface ID (required)
        #[arg(long)]
        surface: u32,
        /// Attention kind — "completion"(기본) 또는 "needs_input". 그 외 값은
        /// "completion"으로 취급된다.
        #[arg(long)]
        kind: Option<String>,
    },
}
