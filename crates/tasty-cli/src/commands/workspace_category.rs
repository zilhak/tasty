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
    Move {
        /// Source index (≥ 1)
        #[arg(long)]
        from: usize,
        /// Destination index (≥ 1)
        #[arg(long)]
        to: usize,
    },
}
