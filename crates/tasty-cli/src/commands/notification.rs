use clap::Subcommand;

#[derive(Subcommand)]
pub enum NotificationCommands {
    /// Count unread notifications by level across all windows
    Counts {
        /// Filter by source workspace ID, including closed workspaces
        #[arg(long)]
        workspace: Option<u32>,
    },
    /// Mark notifications read
    Read {
        /// Notification ID from list notifications
        #[arg(required_unless_present = "all", conflicts_with = "all")]
        id: Option<u64>,
        /// Acknowledge every notification in the selected scope
        #[arg(long)]
        all: bool,
        /// Filter by source workspace ID, including closed workspaces
        #[arg(long)]
        workspace: Option<u32>,
    },
}
