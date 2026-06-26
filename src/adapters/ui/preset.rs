//! `PresetView` 의 egui UI 그리기 함수.
//!
//! 현재는 골격 — 좌측 scope 탭 + 선택 preset 의 **데모 레이아웃 미리보기**.
//! 전체 list→toolbar→detail 셸(2-depth)과 편집 모드는 TODO 08/09 후속이다.
//! `settings_ui::draw_settings_panel` 과 동일하게 외부에서 함수 한 개로 호출.

use tasty_presets::{PresetKind, PresetStore};

use crate::i18n::t;

pub mod demo_layout;

use demo_layout::{DemoLayout, fallback_kind_label};

/// 선택된 preset 으로부터 미리보기 위젯을 만든다. 선택이 없으면 목록 첫 항목.
fn build_demo(store: &PresetStore, kind: PresetKind, name: &str) -> Option<DemoLayout> {
    match kind {
        PresetKind::Workspace => store
            .get_workspace(name)
            .map(|p| DemoLayout::from_workspace(p, fallback_kind_label)),
        PresetKind::Tab => store
            .get_tab(name)
            .map(|p| DemoLayout::from_tab(p, fallback_kind_label)),
        PresetKind::Pane => store
            .get_pane(name)
            .map(|p| DemoLayout::from_pane(p, fallback_kind_label)),
    }
}

/// PresetView 의 본문을 그린다.
pub fn draw_preset_panel(
    ctx: &egui::Context,
    store: &mut PresetStore,
    active_kind: &mut PresetKind,
    selected_workspace: &mut Option<String>,
    selected_tab: &mut Option<String>,
    selected_pane: &mut Option<String>,
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

        let names = store.list(*active_kind);
        let selected = match *active_kind {
            PresetKind::Workspace => selected_workspace,
            PresetKind::Tab => selected_tab,
            PresetKind::Pane => selected_pane,
        };
        // 선택 항목이 유효하면 그것을, 아니면 목록 첫 항목을 미리본다.
        let name = selected
            .clone()
            .filter(|n| names.contains(n))
            .or_else(|| names.first().cloned());

        let Some(name) = name else {
            ui.label(t("preset.popup.empty"));
            return;
        };

        // DemoLayout 은 프레임 간 유지되어야 탭 클릭 전환이 지속된다. 선택 preset
        // 이 바뀌면 다시 빌드한다. egui temp memory 에 (key, layout) 으로 보관.
        let theme = crate::theme::theme();
        let key = format!("{}:{}", active_kind.as_str(), name);
        let cache_id = egui::Id::new("preset_demo_layout_cache");

        let cached: Option<(String, DemoLayout)> = ui.data(|d| d.get_temp(cache_id));
        let mut layout = match cached {
            Some((k, dl)) if k == key => dl,
            _ => match build_demo(store, *active_kind, &name) {
                Some(dl) => dl,
                None => return,
            },
        };

        // 남은 영역에 미리보기 캔버스(테두리 + bg-app + padding) 후 트리 렌더.
        let pad = theme.spacing_sm.value();
        let canvas = ui.available_rect_before_wrap().shrink(pad);
        if canvas.width() > 0.0 && canvas.height() > 0.0 {
            ui.allocate_rect(canvas, egui::Sense::hover());
            let radius = theme.corner_radius.value();
            let bw = theme.border_width.value();
            let p = ui.painter_at(canvas);
            p.rect_filled(canvas, radius, theme.bg_app().to_egui());
            p.rect_stroke(
                canvas,
                radius,
                egui::Stroke::new(bw, theme.border_default().to_egui()),
                egui::StrokeKind::Inside,
            );
            let changed = layout.show(ui, &theme, canvas.shrink(pad));
            if changed {
                ui.ctx().request_repaint();
            }
        }

        ui.data_mut(|d| d.insert_temp(cache_id, (key, layout)));
    });
}
