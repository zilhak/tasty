//! 갤러리 egui shell: 상단 툴바 + 좌측 카탈로그 + 우측 디테일.

use tasty_type_appearance::theme::Theme;

use crate::catalog::{self, CatalogItem, Category};

/// 갤러리에 노출하는 빌트인 테마 식별자.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeId {
    Mocha,
    Latte,
}

impl ThemeId {
    fn label(self) -> &'static str {
        match self {
            ThemeId::Mocha => "Mocha",
            ThemeId::Latte => "Latte",
        }
    }

    fn all() -> &'static [ThemeId] {
        &[ThemeId::Mocha, ThemeId::Latte]
    }

    /// 해당 id 의 *기본 색상 세트* 를 만들고 base mode 를 적용해 Theme 으로 반환.
    fn build(self, is_light: bool) -> Theme {
        let mut t = match self {
            ThemeId::Mocha => tasty_themes::mocha_fallback(),
            ThemeId::Latte => latte_theme(),
        };
        t.set_is_light(is_light);
        t
    }
}

/// 빌트인 `LATTE_TOML_TEXT` 를 Mocha base 위에 partial 로 적용해 Theme 생성.
/// `mocha_fallback()` 과 대칭되는 helper. 실패는 갤러리에서 panic 해도 무방
/// (TOML 텍스트는 crate 에 박혀 있음).
fn latte_theme() -> Theme {
    use tasty_type_appearance::theme::Theme as ThemeStruct;
    let file = tasty_themes::ThemeFile::parse(tasty_themes::LATTE_TOML_TEXT)
        .expect("builtin latte.toml ships valid");
    let (partial, is_light) = file.to_partial();
    let mut colors = tasty_themes::mocha_fallback_colors();
    colors.apply_partial(&partial);
    ThemeStruct::with_colors(colors, is_light.unwrap_or(true))
}

/// 갤러리 전역 상태.
pub struct GalleryState {
    /// 현재 적용 중인 테마. 본체 글로벌 (`tasty_themes::theme()`) 과 격리된
    /// 갤러리 전용 인스턴스 — dropdown 토글이 본체 색에 영향을 주지 않는다.
    pub theme: Theme,
    /// 콤보에서 선택된 테마 식별자. `theme` 자체에는 보존되지 않는 정보
    /// (Theme 구조체는 id 를 모름) 라 별도 보관.
    pub theme_id: ThemeId,
    pub items: Vec<CatalogItem>,
    /// 현재 선택된 카탈로그 항목의 *전역* index (`items` 기준).
    pub selected: usize,
    /// 현재 활성 1차 카테고리. 좌측 사이드바는 이 카테고리의 항목만 표시.
    pub active_category: Category,
    pub ui_scale: f32,
    /// "Apply theme" 후 다음 frame 에서 ctx 에 visuals/style 을 다시 박을지.
    pub needs_reapply: bool,
}

impl GalleryState {
    pub fn new() -> Self {
        let items = catalog::all();
        // 첫 항목의 카테고리를 active 로 두어 진입 시 빈 사이드바를 피한다.
        let active_category = items
            .first()
            .map(|i| i.category)
            .unwrap_or(Category::Foundations);
        let theme_id = ThemeId::Mocha;
        let theme = theme_id.build(false);
        Self {
            theme,
            theme_id,
            items,
            selected: 0,
            active_category,
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
            // Theme 선택 (Mocha / Latte). base mode (light/dark) 토글은 옆 별도.
            ui.label("Theme:");
            egui::ComboBox::from_id_salt("theme_combo")
                .selected_text(state.theme_id.label())
                .show_ui(ui, |ui| {
                    for id in ThemeId::all() {
                        let selected = state.theme_id == *id;
                        if ui.selectable_label(selected, id.label()).clicked() && !selected {
                            state.theme_id = *id;
                            state.theme = id.build(state.theme.is_light);
                            state.needs_reapply = true;
                        }
                    }
                });

            ui.label("Base:");
            for (label, is_light) in [("Dark", false), ("Light", true)] {
                let selected = state.theme.is_light == is_light;
                if ui.selectable_label(selected, label).clicked() && !selected {
                    state.theme.set_is_light(is_light);
                    state.needs_reapply = true;
                }
            }

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

        // 1차 카테고리 탭 — Appearance / Widget / Popup / Component / Layout.
        ui.horizontal(|ui| {
            for category in Category::all() {
                let count = state
                    .items
                    .iter()
                    .filter(|i| i.category == *category)
                    .count();
                let label = format!("{} ({count})", category.label());
                if ui
                    .selectable_label(state.active_category == *category, label)
                    .clicked()
                {
                    state.active_category = *category;
                    // 새 카테고리의 첫 항목으로 자동 선택 이동.
                    if let Some((idx, _)) = state
                        .items
                        .iter()
                        .enumerate()
                        .find(|(_, item)| item.category == *category)
                    {
                        state.selected = idx;
                    }
                }
            }
        });
        ui.add_space(2.0);
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
                        if item.category != state.active_category {
                            continue;
                        }
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
