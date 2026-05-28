//! `PresetStorage` trait — Hexagonal architecture 의 *internal port*.
//!
//! `PresetStore` (instance) 가 자체 impl. Core 가 `Arc<Mutex<dyn PresetStorage>>`
//! 보유. test 시 `testing::InMemoryPresetStorage`.

use crate::model::{PanePreset, PresetKind, TabPreset, WorkspacePreset};
use crate::storage::PresetResult;

/// Preset disk store 의 동작 인터페이스.
pub trait PresetStorage: Send + Sync {
    fn list(&self, kind: PresetKind) -> Vec<String>;

    fn get_workspace(&self, name: &str) -> Option<&WorkspacePreset>;
    fn get_tab(&self, name: &str) -> Option<&TabPreset>;
    fn get_pane(&self, name: &str) -> Option<&PanePreset>;

    fn save_workspace(&mut self, preset: WorkspacePreset) -> PresetResult<()>;
    fn save_tab(&mut self, preset: TabPreset) -> PresetResult<()>;
    fn save_pane(&mut self, preset: PanePreset) -> PresetResult<()>;

    fn save_workspace_overwrite(&mut self, preset: WorkspacePreset) -> PresetResult<()>;
    fn save_tab_overwrite(&mut self, preset: TabPreset) -> PresetResult<()>;
    fn save_pane_overwrite(&mut self, preset: PanePreset) -> PresetResult<()>;

    fn delete(&mut self, kind: PresetKind, name: &str) -> PresetResult<()>;
    fn rename(&mut self, kind: PresetKind, from: &str, to: &str) -> PresetResult<()>;

    fn unique_name(&self, kind: PresetKind, base: &str) -> String;
}
