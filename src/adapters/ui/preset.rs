//! `PresetView` 의 egui UI 그리기 함수.
//!
//! 현재는 골격 — 탭 전환만 있는 placeholder. 실제 리스트/편집 폼은 다음 stage 에서
//! 채운다. `settings_ui::draw_settings_panel` 과 동일하게 외부에서 함수 한 개로 호출.

use tasty_presets::{PresetKind, PresetStore};

use crate::i18n::t;

/// PresetView 의 본문을 그린다.
pub fn draw_preset_panel(
    ctx: &egui::Context,
    store: &mut PresetStore,
    active_kind: &mut PresetKind,
    _selected_workspace: &mut Option<String>,
    _selected_tab: &mut Option<String>,
    _selected_pane: &mut Option<String>,
) {
    egui::CentralPanel::default().show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.selectable_value(
                active_kind,
                PresetKind::Workspace,
                t("preset.tab.workspace"),
            );
            ui.selectable_value(active_kind, PresetKind::Tab, t("preset.tab.tab"));
            ui.selectable_value(active_kind, PresetKind::Pane, t("preset.tab.pane"));
        });
        ui.separator();

        let count = store.list(*active_kind).len();
        ui.label(t("preset.list.placeholder"));
        ui.label(format!("count: {count}"));
    });
}
