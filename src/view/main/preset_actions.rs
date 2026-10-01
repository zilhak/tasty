//! Capture requests bind structural IDs and View choices; App owns kind snapshot and file execution.
use super::MainView;
use crate::{intent::Intent, runtime::engine_read::EngineRead};
use anyhow::{Result, anyhow};
impl MainView {
    fn request_preset_capture(
        &mut self,
        engine: &EngineRead<'_>,
        kind: tasty_presets::PresetKind,
        source: u32,
    ) {
        let presentation = crate::model::StructurePresentationSnapshot::capture(
            engine.workspaces(),
            engine.categories(),
            &self.state.navigation,
        );
        self.state.dispatch_intent(
            Intent::CapturePreset {
                kind,
                source,
                presentation,
            }
            .from_user_context_menu(),
        );
    }
    pub(crate) fn save_workspace_preset_from_idx(
        &mut self,
        engine: &EngineRead<'_>,
        index: usize,
    ) -> Result<()> {
        let source = engine
            .workspace_at(index)
            .ok_or_else(|| anyhow!("workspace index {index} not found"))?
            .id;
        self.request_preset_capture(engine, tasty_presets::PresetKind::Workspace, source);
        Ok(())
    }
    pub(crate) fn save_tab_preset_from_pane_tab(
        &mut self,
        engine: &EngineRead<'_>,
        pane: u32,
        index: usize,
    ) -> Result<()> {
        let source = engine
            .find_pane_by_id(pane)
            .and_then(|pane| pane.tabs.get(index))
            .ok_or_else(|| anyhow!("tab index {index} not found in pane {pane}"))?
            .id;
        self.request_preset_capture(engine, tasty_presets::PresetKind::Tab, source);
        Ok(())
    }
    pub(crate) fn save_pane_preset_from_pane_id(
        &mut self,
        engine: &EngineRead<'_>,
        pane: u32,
    ) -> Result<()> {
        if engine.find_pane_by_id(pane).is_none() {
            return Err(anyhow!("pane {pane} not found"));
        }
        self.request_preset_capture(engine, tasty_presets::PresetKind::Pane, pane);
        Ok(())
    }
}
