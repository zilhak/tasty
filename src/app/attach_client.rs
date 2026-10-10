//! App integration for remote mirrors; each child imports its actual collaborators.
mod agent_origin;
pub(crate) mod attempts;
mod bulk;
mod connection;
mod dispatch;
mod forward;
mod home_probe;
pub(crate) mod into_gui;
mod navigation;
mod output;
pub(crate) mod pending;
mod projection;
mod resize_sync;
mod resources;
mod survivors;
mod wire;

#[cfg(test)]
mod tests;

/// 번들 git-viewer 매니페스트의 ID와 일치해야 한다.
const GIT_VIEWER_PLUGIN_ID: &str = "com.tasty.git-viewer";
const GIT_VIEWER_QUERY_RESULT_EVENT: &str = "git_viewer.query_result";

pub(crate) use bulk::{BULK_REJECT_PREFIX, upload_file_over_bulk};
pub(super) use connection::find_parked_with_workspace;
pub(crate) use forward::RemoteTarget;
