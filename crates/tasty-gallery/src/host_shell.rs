//! 갤러리 egui shell: 상단 툴바 + 좌측 카탈로그 + 우측 디테일.

use tasty_type_appearance::theme::Theme;

use crate::catalog::{self, CatalogItem};

/// 갤러리 전역 상태.
pub struct GalleryState {
    /// 현재 적용 중인 테마. 본체 글로벌 (`tasty_themes::theme()`) 과 격리된
    /// 갤러리 전용 인스턴스 — dropdown 토글이 본체 색에 영향을 주지 않는다.
    pub theme: Theme,
    pub items: Vec<CatalogItem>,
    pub selected: usize,
    pub ui_scale: f32,
    /// "Apply theme" 후 다음 frame 에서 ctx 에 visuals/style 을 다시 박을지.
    pub needs_reapply: bool,
}

impl GalleryState {
    pub fn new() -> Self {
        Self {
            theme: tasty_themes::mocha_fallback(),
            items: catalog::all(),
            selected: 0,
            ui_scale: 1.0,
            needs_reapply: true,
        }
    }
}

impl Default for GalleryState {
    fn default() -> Self {
        Self::new()
    }
}

/// 한 frame draw. `apply_theme_to_egui` 는 toolbar 가 theme/ui_scale 을 바꿨을
/// 때만 호출하면 충분하지만, 단순화 위해 매 frame 호출해도 비용 무시 가능.
pub fn draw(ctx: &egui::Context, state: &mut GalleryState) {
    if state.needs_reapply {
        tasty_egui_theme::apply_theme_to_egui(&state.theme, ctx, state.ui_scale);
        state.needs_reapply = false;
    }

    egui::TopBottomPanel::top("gallery_toolbar").show(ctx, |ui| {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label("Theme:");
            let label = if state.theme.is_light {
                "Mocha (light base)"
            } else {
                "Mocha (dark base)"
            };
            egui::ComboBox::from_id_salt("theme_combo")
                .selected_text(label)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(!state.theme.is_light, "Mocha (dark base)")
                        .clicked()
                        && state.theme.is_light
                    {
                        state.theme.set_is_light(false);
                        state.needs_reapply = true;
                    }
                    if ui
                        .selectable_label(state.theme.is_light, "Mocha (light base)")
                        .clicked()
                        && !state.theme.is_light
                    {
                        state.theme.set_is_light(true);
                        state.needs_reapply = true;
                    }
                });

            ui.separator();
            ui.label("UI scale:");
            // 본체 ui_scale_factor 매핑 (src/../appearance.rs) 와 동일: 0.85 / 1.0 / 1.2.
            for (label, scale) in [("Small", 0.85_f32), ("Medium", 1.0), ("Large", 1.2)] {
                let selected = (state.ui_scale - scale).abs() < 0.001;
                if ui.selectable_label(selected, label).clicked() && !selected {
                    state.ui_scale = scale;
                    state.needs_reapply = true;
                }
            }

            ui.separator();
            ui.label("(i18n: gallery 자체는 단순 표기, 본체 i18n 시스템 미사용)");
        });
        ui.add_space(4.0);
    });

    egui::SidePanel::left("gallery_sidebar")
        .resizable(true)
        .default_width(240.0)
        .min_width(160.0)
        .show(ctx, |ui| {
            ui.add_space(8.0);
            ui.heading("Catalog");
            ui.add_space(4.0);
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for (idx, item) in state.items.iter().enumerate() {
                        if ui
                            .selectable_label(state.selected == idx, item.name)
                            .clicked()
                        {
                            state.selected = idx;
                        }
                    }
                });
        });

    // CentralPanel 의 우측 inner_margin 을 0 으로 두어 ScrollArea 의 스크롤바가
    // 갤러리 창 우측 끝에 정확히 붙도록 한다. 상/좌/하 margin 은 기본값 유지.
    let central_frame = egui::Frame::central_panel(&ctx.style()).inner_margin(egui::Margin {
        left: 8,
        right: 0,
        top: 8,
        bottom: 8,
    });
    egui::CentralPanel::default()
        .frame(central_frame)
        .show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let item = state.items.get(state.selected).copied();
                    if let Some(item) = item {
                        ui.heading(item.name);
                        ui.separator();
                        (item.draw)(ui, &state.theme);
                    } else {
                        ui.label("No catalog item selected.");
                    }
                });
        });
}
