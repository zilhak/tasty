use clap::Subcommand;

/// `tasty memory goal ...` 서브커맨드.
///
/// `--surface` 는 optional — 생략 시 caller 의 `TASTY_SURFACE_ID` env 로 해석한다
/// (에이전트가 자기 자신에 대해 호출하는 것이 주 용례). 둘 다 없으면 에러.
#[derive(Subcommand)]
pub enum MemoryGoalCommands {
    /// Set the surface's goal (overwrites any existing one). Blank goals are rejected.
    Set {
        /// Goal sentence.
        goal: String,
        /// Target surface ID (defaults to caller's TASTY_SURFACE_ID)
        #[arg(long)]
        surface: Option<u32>,
    },
    /// Read the surface's goal (returns null if unset).
    Get {
        /// Target surface ID (defaults to caller's TASTY_SURFACE_ID)
        #[arg(long)]
        surface: Option<u32>,
    },
    /// Remove the surface's goal (idempotent).
    Clear {
        /// Target surface ID (defaults to caller's TASTY_SURFACE_ID)
        #[arg(long)]
        surface: Option<u32>,
    },
}
