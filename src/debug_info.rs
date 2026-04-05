//! Developer-local debug info collector.
//!
//! This file is meant to be freely modified by each developer for their own
//! debugging needs. After the initial commit, run:
//!
//!   git update-index --skip-worktree src/debug_info.rs
//!
//! to prevent local changes from appearing in `git status` or being committed.
//! To undo: `git update-index --no-skip-worktree src/debug_info.rs`

use serde_json::{json, Value};

use crate::state::AppState;
use crate::gpu::GpuState;

/// Collect debug information from the running tasty instance.
/// Modify this function freely — add whatever you need to diagnose issues.
pub fn collect(state: &AppState, gpu: Option<&GpuState>) -> Value {
    let mut info = serde_json::Map::new();

    // -- Basic state --
    info.insert("workspace_count".into(), json!(state.engine.workspaces.len()));
    info.insert("active_workspace".into(), json!(state.active_workspace));

    // -- GPU / scale factor --
    if let Some(gpu) = gpu {
        info.insert("scale_factor".into(), json!(gpu.scale_factor()));
        info.insert("cell_width".into(), json!(gpu.cell_width()));
        info.insert("cell_height".into(), json!(gpu.cell_height()));
        let size = gpu.size();
        info.insert("viewport_width".into(), json!(size.width));
        info.insert("viewport_height".into(), json!(size.height));
    }

    // -- Font settings --
    info.insert("font_size".into(), json!(state.engine.settings.appearance.font_size));
    info.insert("font_family".into(), json!(&state.engine.settings.appearance.font_family));

    // -- Add your own debug info below this line --

    Value::Object(info)
}
