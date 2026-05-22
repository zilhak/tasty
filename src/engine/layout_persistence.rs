//! Layout persistence: save/restore workspace layout to `~/.tasty/layout.json`.
//!
//! Captures the structural tree (workspaces → pane nodes → panes → tabs → surface layouts)
//! with minimal per-surface info (cwd, file path, url). No screen/scrollback content.
//!
//! `SavedSurface` is `Terminal` + `Generic { kind, data }`. New surface kinds (including
//! plugins) round-trip via the SurfaceKindRegistry without touching this file.

mod capture;
mod restore;
mod schema;
mod scrollback;
#[cfg(test)]
mod tests;

use std::path::PathBuf;
use std::time::Instant;

pub use schema::SavedLayout;

use crate::engine_state::EngineState;

const DEBOUNCE_MS: u128 = 500;
pub(super) const LAYOUT_VERSION: u32 = 2;

// ── Disk I/O ──

fn layout_path() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|dirs| dirs.home_dir().join(".tasty").join("layout.json"))
}

/// Save layout to disk. Non-blocking best-effort.
///
/// `&mut EngineState` 인 이유: `SavedLayout::capture` 가 새 persist_id 를 발급하면
/// 해당 surface 인스턴스의 `scrollback_persist_id` 필드에 기록해 다음 capture 가
/// 같은 ID 를 재사용한다.
pub fn save_to_disk(engine: &mut EngineState, active_workspace: usize) {
    let path = match layout_path() {
        Some(p) => p,
        None => {
            tracing::warn!("Cannot determine ~/.tasty path for layout save");
            return;
        }
    };
    let saved = SavedLayout::capture(engine, active_workspace);
    let json = match serde_json::to_string_pretty(&saved) {
        Ok(j) => j,
        Err(e) => {
            tracing::warn!("Failed to serialize layout: {e}");
            return;
        }
    };
    if let Some(parent) = path.parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        tracing::warn!("Failed to create dir for layout.json: {e}");
        return;
    }
    if let Err(e) = std::fs::write(&path, json) {
        tracing::warn!("Failed to write layout.json: {e}");
    }
}

/// Load layout from disk. Returns None if file doesn't exist or is invalid.
pub fn load_from_disk() -> Option<SavedLayout> {
    let path = layout_path()?;
    let json = std::fs::read_to_string(&path).ok()?;
    match serde_json::from_str::<SavedLayout>(&json) {
        Ok(layout) => {
            if layout.version > LAYOUT_VERSION {
                tracing::warn!(
                    "layout.json version {} is newer than supported {}",
                    layout.version,
                    LAYOUT_VERSION
                );
                return None;
            }
            Some(layout)
        }
        Err(e) => {
            tracing::warn!("Failed to parse layout.json: {e}");
            None
        }
    }
}

// ── Dirty flag / debounce state ──

/// Tracks whether the layout has been modified and needs saving.
pub struct LayoutDirtyTracker {
    dirty: bool,
    dirty_since: Option<Instant>,
}

impl LayoutDirtyTracker {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Default for LayoutDirtyTracker {
    fn default() -> Self {
        Self {
            dirty: false,
            dirty_since: None,
        }
    }
}

impl LayoutDirtyTracker {
    /// Mark layout as dirty (called on structural changes).
    pub fn mark_dirty(&mut self) {
        if !self.dirty {
            self.dirty = true;
            self.dirty_since = Some(Instant::now());
        }
    }

    /// Check if enough time has elapsed and a flush is needed.
    /// Returns true if the caller should save now.
    pub fn should_flush(&self) -> bool {
        if !self.dirty {
            return false;
        }
        match self.dirty_since {
            Some(since) => since.elapsed().as_millis() >= DEBOUNCE_MS,
            None => false,
        }
    }

    /// Reset after a successful save.
    pub fn clear(&mut self) {
        self.dirty = false;
        self.dirty_since = None;
    }

    /// Force check if dirty (for shutdown flush).
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }
}
