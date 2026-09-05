//! `tasty workspace-category` subcommand 정의 — 사이드바 폴더(카테고리) CRUD.
//!
//! 카테고리 *CRUD·reorder* 는 에이전트 작업이라 CLI/IPC 양면 노출(원칙 2). 선택/접힘
//! 같은 사용자 UI 상태는 노출하지 않는다(원칙 1·3).

use clap::Subcommand;

#[derive(Subcommand)]
pub enum WorkspaceCategoryCommands {
    /// List workspace categories (sidebar folders)
    List,
    /// Create a new category
    Create {
        /// Category name ('normal' is reserved; case-insensitive duplicates rejected)
        #[arg(long)]
        name: String,
    },
    /// Rename a category (the 'normal' category cannot be renamed)
    Rename {
        /// Category ID (required)
        #[arg(long)]
        id: u32,
        /// New name
        #[arg(long)]
        name: String,
    },
    /// Delete a category; its workspaces move to 'normal' (the 'normal' category cannot be deleted)
    Delete {
        /// Category ID (required)
        #[arg(long)]
        id: u32,
    },
    /// Reorder categories ('normal' is fixed at index 0; from/to must be ≥ 1)
    ///
    /// Prefer `--id`: a category ID names the window that owns it, so the command
    /// means the same thing no matter which window is focused. `--from` is an index
    /// inside one window, so it lands on whichever window has focus.
    Move {
        /// Category ID to move (preferred; mutually exclusive with --from)
        #[arg(long, conflicts_with = "from")]
        id: Option<u32>,
        /// Source index (≥ 1, focus-dependent)
        #[arg(long)]
        from: Option<usize>,
        /// Destination index (≥ 1)
        #[arg(long)]
        to: usize,
    },
}
