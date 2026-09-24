//! 현재 워크스페이스·탭·패널 구성을 읽어 Intent::SavePreset으로 보낸다.
//! 저장 이름 결정, 파일 저장, 프리셋 창 열기는 src/intent/preset.rs에서 처리한다.

use anyhow::{Result, anyhow};

use crate::intent::preset_capture::{
    capture_pane_preset, capture_tab_preset, capture_workspace_preset,
};
use crate::intent::{ClonedPreset, Intent};

use super::MainView;

impl MainView {
    pub(crate) fn save_workspace_preset_from_idx(&mut self, ws_idx: usize) -> Result<()> {
        let engine = &mut self.core_state;
        let ws = engine
            .workspaces
            .get(ws_idx)
            .ok_or_else(|| anyhow!("workspace idx {ws_idx} out of range"))?;
        let base_name = if ws.name.is_empty() {
            "workspace".to_string()
        } else {
            ws.name.clone()
        };

        let registry = engine.surface_registry.clone();
        let preset = capture_workspace_preset(engine, ws, None, &registry)
            .ok_or_else(|| anyhow!("workspace capture failed"))?;

        self.state.dispatch_intent(
            Intent::SavePreset {
                base_name,
                explicit_name: None,
                overwrite: false,
                preset: ClonedPreset::Workspace(preset),
            }
            .from_user_context_menu(),
        );
        Ok(())
    }

    pub(crate) fn save_tab_preset_from_pane_tab(
        &mut self,
        pane_id: u32,
        tab_index: usize,
    ) -> Result<()> {
        let engine = &mut self.core_state;
        let ws = self.state.active_workspace(engine);
        let pane = ws
            .pane_layout()
            .find_pane(pane_id)
            .ok_or_else(|| anyhow!("pane {pane_id} not found"))?;
        let tab = pane
            .tabs
            .get(tab_index)
            .ok_or_else(|| anyhow!("tab idx {tab_index} out of range"))?;
        let base = tab
            .explicit_name
            .clone()
            .unwrap_or_else(|| tab.name.clone());
        let base_name = if base.is_empty() {
            "tab".to_string()
        } else {
            base
        };

        let registry = engine.surface_registry.clone();
        let preset = capture_tab_preset(engine, tab, None, &registry)
            .ok_or_else(|| anyhow!("tab capture failed"))?;

        self.state.dispatch_intent(
            Intent::SavePreset {
                base_name,
                explicit_name: None,
                overwrite: false,
                preset: ClonedPreset::Tab(preset),
            }
            .from_user_context_menu(),
        );
        Ok(())
    }

    pub(crate) fn save_pane_preset_from_pane_id(&mut self, pane_id: u32) -> Result<()> {
        let engine = &mut self.core_state;
        let ws = self.state.active_workspace(engine);
        let pane = ws
            .pane_layout()
            .find_pane(pane_id)
            .ok_or_else(|| anyhow!("pane {pane_id} not found"))?;
        let base_name = "pane".to_string();

        let registry = engine.surface_registry.clone();
        let preset = capture_pane_preset(engine, pane, None, &registry)
            .ok_or_else(|| anyhow!("pane capture failed"))?;

        self.state.dispatch_intent(
            Intent::SavePreset {
                base_name,
                explicit_name: None,
                overwrite: false,
                preset: ClonedPreset::Pane(preset),
            }
            .from_user_context_menu(),
        );
        Ok(())
    }
}
