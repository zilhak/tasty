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
pub fn collect(state: &AppState, gpu: Option<&GpuState>, ime_active: bool) -> Value {
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

    // -- Font settings (per-surface effective values) --
    let appearance = &state.engine.settings.appearance;
    let term_eff = appearance.effective_terminal_font();
    let md_eff = appearance.effective_markdown_font();
    let exp_eff = appearance.effective_explorer_font();
    info.insert("default_font_size".into(), json!(appearance.default_font.font_size));
    info.insert("default_font_family".into(), json!(&appearance.default_font.font_family));
    info.insert("terminal_font_size".into(), json!(term_eff.font_size));
    info.insert("terminal_font_family".into(), json!(term_eff.font_family));
    info.insert("markdown_font_size".into(), json!(md_eff.font_size));
    info.insert("markdown_font_family".into(), json!(md_eff.font_family));
    info.insert("explorer_font_size".into(), json!(exp_eff.font_size));
    info.insert("explorer_font_family".into(), json!(exp_eff.font_family));

    // -- IME state --
    info.insert("ime_active".into(), json!(ime_active));
    if let Some(gpu) = gpu {
        info.insert("egui_ime_allowed".into(), json!(gpu.egui_ime_allowed()));
    }

    // -- Add your own debug info below this line --

    // -- egui actual rendering state (may differ from gpu.scale_factor) --
    if let Some(gpu) = gpu {
        info.insert("egui_pixels_per_point".into(), json!(gpu.egui_pixels_per_point()));
        info.insert("egui_zoom_factor".into(), json!(gpu.egui_zoom_factor()));
        let (cfg_w, cfg_h) = gpu.surface_config_size();
        info.insert("surface_config_width".into(), json!(cfg_w));
        info.insert("surface_config_height".into(), json!(cfg_h));
    }

    // -- tab bar height (physical px, measured by egui) --
    info.insert("tab_bar_height".into(), json!(state.tab_bar_height));
    info.insert("sidebar_width".into(), json!(state.sidebar_width));

    Value::Object(info)
}
