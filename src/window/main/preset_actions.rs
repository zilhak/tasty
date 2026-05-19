//! 우클릭 메뉴 진입점 — Workspace/Tab/Pane 을 캡처해 preset 저장 큐로 넘긴다.
//!
//! 본 모듈은 capture 만 수행한다. `PresetStore` 는 App 레벨 (engine::Engine) 에서 관리되므로
//! `state.dialogs.pending_preset_save` 1슬롯 큐로 App 메인 루프에 넘긴다.
//! App 이 unique_name 부여 → save_* → PresetWindow 오픈 + select 까지 일괄 처리한다.

use anyhow::{Result, anyhow};
use tasty_presets::{
    CaptureOptions, CapturedSurfaceMeta, PanePreset, TabPreset, WorkspacePreset,
};

use crate::model::Surface;
use crate::state::PendingPresetSave;

use super::MainWindow;

impl MainWindow {
    pub(crate) fn save_workspace_preset_from_idx(&mut self, ws_idx: usize) -> Result<()> {
        let ws = self
            .state
            .engine
            .workspaces
            .get(ws_idx)
            .ok_or_else(|| anyhow!("workspace idx {ws_idx} out of range"))?;
        let base_name = if ws.name.is_empty() {
            "workspace".to_string()
        } else {
            ws.name.clone()
        };

        let registry = self.state.engine.surface_registry.clone();
        let mut capture = move |s: &dyn Surface| -> Option<CapturedSurfaceMeta> {
            let def = registry.get(s.kind())?;
            let params = (def.snapshot)(s)?;
            Some(CapturedSurfaceMeta {
                kind: s.kind().to_string(),
                params,
            })
        };
        let preset = WorkspacePreset::from_workspace(ws, &mut capture, CaptureOptions::default())
            .ok_or_else(|| anyhow!("workspace capture failed"))?;

        self.state.dialogs.pending_preset_save =
            Some(PendingPresetSave::Workspace { base_name, preset });
        Ok(())
    }

    pub(crate) fn save_tab_preset_from_pane_tab(
        &mut self,
        pane_id: u32,
        tab_index: usize,
    ) -> Result<()> {
        let ws = self.state.active_workspace();
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
        let base_name = if base.is_empty() { "tab".to_string() } else { base };

        let registry = self.state.engine.surface_registry.clone();
        let mut capture = move |s: &dyn Surface| -> Option<CapturedSurfaceMeta> {
            let def = registry.get(s.kind())?;
            let params = (def.snapshot)(s)?;
            Some(CapturedSurfaceMeta {
                kind: s.kind().to_string(),
                params,
            })
        };
        let preset = TabPreset::from_tab(tab, &mut capture, CaptureOptions::default())
            .ok_or_else(|| anyhow!("tab capture failed"))?;

        self.state.dialogs.pending_preset_save =
            Some(PendingPresetSave::Tab { base_name, preset });
        Ok(())
    }

    pub(crate) fn save_pane_preset_from_pane_id(&mut self, pane_id: u32) -> Result<()> {
        let ws = self.state.active_workspace();
        let pane = ws
            .pane_layout()
            .find_pane(pane_id)
            .ok_or_else(|| anyhow!("pane {pane_id} not found"))?;
        let base_name = "pane".to_string();

        let registry = self.state.engine.surface_registry.clone();
        let mut capture = move |s: &dyn Surface| -> Option<CapturedSurfaceMeta> {
            let def = registry.get(s.kind())?;
            let params = (def.snapshot)(s)?;
            Some(CapturedSurfaceMeta {
                kind: s.kind().to_string(),
                params,
            })
        };
        let preset = PanePreset::from_pane(pane, &mut capture, CaptureOptions::default())
            .ok_or_else(|| anyhow!("pane capture failed"))?;

        self.state.dialogs.pending_preset_save =
            Some(PendingPresetSave::Pane { base_name, preset });
        Ok(())
    }
}
