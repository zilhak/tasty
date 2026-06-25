//! `tabbar` specimen — Pane tab strip (research §2.5 Layouts).
//!
//! 한 pane 안의 탭 줄. tab 24×150, strip 은 bg-sidebar + 하단 border.
//! Tab×3 + `+` IconButton, 우측에 Split / Search. 활성 탭은 bg-panel +
//! accent top bar 로 구분. 본체 `src/adapters/ui/tab_bar.rs` 의 시각 패턴을
//! Theme 토큰만으로 정적 재현 (binary 미의존).

use tasty_type_appearance::theme::Theme;

use crate::catalog::icons::{PLUS, SEARCH, SPLIT};
use crate::catalog::spec::{self, StageVariant, TokenChip};

const TABS: &[(&str, bool)] = &[("README.md", false), ("build.rs", true), ("run.rs", false)];

fn strip(ui: &mut egui::Ui, theme: &Theme) {
    let bar_h = theme.item_height_tab.value(); // 24
    let tab_w = theme.tab_width.value(); // 150
    let w = ui.available_width().min(theme.measure_xl.value());
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, bar_h), egui::Sense::hover());
    let p = ui.painter_at(rect);

    // strip bg-sidebar + 하단 border.
    p.rect_filled(rect, 0.0, egui::Color32::from(theme.bg_sidebar()));
    p.hline(
        rect.x_range(),
        rect.max.y - theme.border_width.value() * 0.5,
        egui::Stroke::new(theme.border_width.value(), egui::Color32::from(theme.border_default())),
    );

    let font = egui::FontId::proportional(theme.tab_bar_label_font_size.value());
    let mut x = rect.min.x;
    for (i, (name, active)) in TABS.iter().enumerate() {
        let tab = egui::Rect::from_min_size(egui::pos2(x, rect.min.y), egui::vec2(tab_w, bar_h));
        if *active {
            p.rect_filled(tab, 0.0, egui::Color32::from(theme.bg_panel()));
            let bar = egui::Rect::from_min_size(
                tab.min,
                egui::vec2(tab_w, theme.tab_indicator_width.value()),
            );
            p.rect_filled(bar, 0.0, egui::Color32::from(theme.accent_primary()));
        }
        if i > 0 {
            p.vline(
                x,
                rect.y_range(),
                egui::Stroke::new(theme.border_width.value(), egui::Color32::from(theme.separator)),
            );
        }
        p.text(
            egui::pos2(tab.min.x + theme.spacing_sm.value(), tab.center().y),
            egui::Align2::LEFT_CENTER,
            name,
            font.clone(),
            egui::Color32::from(if *active {
                theme.text_primary()
            } else {
                theme.text_muted()
            }),
        );
        x += tab_w;
    }

    // `+` IconButton (탭 뒤).
    let icon = theme.icon_glyph_size_md.value();
    let plus_rect = egui::Rect::from_min_size(egui::pos2(x, rect.min.y), egui::vec2(bar_h, bar_h));
    paint_icon(ui, PLUS, plus_rect, icon, egui::Color32::from(theme.text_secondary()));

    // 우측: Split + Search.
    let search_rect = egui::Rect::from_min_size(
        egui::pos2(rect.max.x - bar_h, rect.min.y),
        egui::vec2(bar_h, bar_h),
    );
    let split_rect = search_rect.translate(egui::vec2(-bar_h, 0.0));
    paint_icon(ui, SPLIT, split_rect, icon, egui::Color32::from(theme.text_secondary()));
    paint_icon(ui, SEARCH, search_rect, icon, egui::Color32::from(theme.text_secondary()));
}

fn paint_icon(
    ui: &mut egui::Ui,
    glyph: crate::catalog::icons::MockGlyph,
    area: egui::Rect,
    size: f32,
    color: egui::Color32,
) {
    let icon_rect = egui::Rect::from_center_size(area.center(), egui::vec2(size, size));
    glyph.image(size, color).paint_at(ui, icon_rect);
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Solo, |ui| {
        strip(ui, theme);
    });

    spec::meta(
        ui,
        theme,
        &[
            ("tab-height", "24 (control-height-tab)"),
            ("tab-width", "150"),
            ("strip", "bg-sidebar + bottom border"),
            ("active", "bg-panel + accent top bar"),
            ("controls", "+ · Split · Search"),
        ],
        &[
            TokenChip::new("bg-sidebar", "strip fill", theme.bg_sidebar().into()),
            TokenChip::new("bg-panel", "active tab", theme.bg_panel().into()),
            TokenChip::new("accent-primary", "active top bar", theme.accent_primary().into()),
            TokenChip::new("separator", "tab divider", theme.separator.into()),
        ],
    );

    spec::note(
        ui,
        theme,
        "활성 탭만 bg-panel 로 떠오르고 상단에 2px accent bar — 강조는 색이 아니라 \
         '바닥에서 들어올린' elevation 으로 준다.",
    );
}
