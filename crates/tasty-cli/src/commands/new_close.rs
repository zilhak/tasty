//! `tasty new` / `tasty close` subcommand 정의.

use clap::Subcommand;

#[derive(Subcommand)]
pub enum NewCommands {
    /// Create a new window
    Window,
    /// Create a new workspace
    Workspace {
        /// Name for the new workspace
        #[arg(long)]
        name: Option<String>,
        /// Working directory for the new workspace
        #[arg(long)]
        cwd: Option<String>,
        /// Surface type: terminal (default), markdown, explorer, html, image
        #[arg(long, default_value = "terminal")]
        r#type: String,
        /// File path (for markdown/image type)
        #[arg(long)]
        file: Option<String>,
        /// Directory path (for explorer type)
        #[arg(long)]
        path: Option<String>,
        /// URL (for html type)
        #[arg(long)]
        url: Option<String>,
        /// 단계 7 — 이 워크스페이스를 저장된 SSH 프로필(원격 컴퓨터)에 매핑한다.
        /// 활성화 시 호스트가 자동 attach(SSH 터널 + workspace mirror) 한다.
        #[arg(long)]
        ssh_profile: Option<String>,
        /// 단계 7 — 저장 프로필 없이 1회성 인라인 SSH 대상에 매핑. 예: --ssh user@host.
        /// `--ssh-profile` 과 상호배타.
        #[arg(long)]
        ssh: Option<String>,
        /// 단계 7 — 매핑된 원격 tasty 의 attach 대상 workspace_id (원칙 3 — ID 명시).
        #[arg(long)]
        remote_workspace: Option<u32>,
    },
    /// Create a new tab in the specified pane
    Tab {
        /// Target pane ID (required)
        #[arg(long)]
        pane: u32,
        /// Surface type: terminal (default), markdown, explorer, html, image
        #[arg(long, default_value = "terminal")]
        r#type: String,
        /// Working directory (for terminal type)
        #[arg(long)]
        cwd: Option<String>,
        /// File path (for markdown type)
        #[arg(long)]
        file: Option<String>,
        /// Directory path (for explorer type)
        #[arg(long)]
        path: Option<String>,
        /// URL (for html type)
        #[arg(long)]
        url: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum CloseCommands {
    /// Close a specific tab by its ID
    Tab {
        /// Target tab ID (required)
        #[arg(long)]
        tab: u32,
    },
    /// Close the specified pane (unsplit)
    Pane {
        /// Target pane ID (required)
        #[arg(long)]
        pane: u32,
    },
    /// Close the specified surface within a tab
    Surface {
        /// Target surface ID (required)
        #[arg(long)]
        surface: u32,
    },
    /// Close the calling surface itself (uses TASTY_SURFACE_ID)
    #[command(name = "self")]
    CloseSelf,
}
