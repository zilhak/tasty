//! 탐색기 내부 탭바 예제. 라벨 길이에 맞춘 탭 폭과 상단 활성 표시를 사용한다.

use tasty_type_appearance::theme::Theme;

use crate::catalog::icons::{CLOSE, FOLDER, MockGlyph, PLUS};
use crate::catalog::spec::{self, StageVariant, TokenChip};

/// (label, active)
const TABS: &[(&str, bool)] = &[("Downloads", true), ("src", false), ("target", false)];

fn strip(ui: &mut egui::Ui, theme: &Theme) {
    let bar_h = theme.item_height_tab.value(); // 24
    let pad_x = theme.spacing_sm.value();
    let gap = theme.spacing_xs.value();
    let icon_xs = theme.icon_glyph_size_xs.value(); // 12
    let body = theme.font_size_body.value();
    let font = egui::FontId::proportional(body);

    let w = ui.available_width().min(theme.measure_lg.value());
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, bar_h), egui::Sense::hover());
    let p = ui.painter_at(rect);

    p.rect_filled(rect, 0.0, egui::Color32::from(theme.bg_sidebar()));
    p.hline(
        rect.x_range(),
        rect.max.y - theme.border_width.value() * 0.5,
        egui::Stroke::new(
            theme.border_width.value(),
            egui::Color32::from(theme.border_strong()),
        ),
    );

    let mut x = rect.min.x;
    for (i, (label, active)) in TABS.iter().enumerate() {
        let galley = ui
            .fonts(|f| f.layout_no_wrap((*label).to_string(), font.clone(), egui::Color32::WHITE));
        let tab_w = pad_x + icon_xs + gap + galley.size().x + gap + icon_xs + pad_x;
        let tab_rect =
            egui::Rect::from_min_size(egui::pos2(x, rect.min.y), egui::vec2(tab_w, bar_h));
        let resp = ui.interact(
            tab_rect,
            ui.id().with(("explorer_tab", i)),
            egui::Sense::click(),
        );

        if i > 0 && !active {
            ui.painter().vline(
                x,
                tab_rect.y_range(),
                egui::Stroke::new(
                    theme.border_width.value(),
                    egui::Color32::from(theme.separator),
                ),
            );
        }

        if *active {
            ui.painter()
                .rect_filled(tab_rect, 0.0, egui::Color32::from(theme.bg_panel()));
            let indicator = egui::Rect::from_min_size(
                tab_rect.min,
                egui::vec2(tab_w, theme.tab_indicator_width.value()),
            );
            ui.painter()
                .rect_filled(indicator, 0.0, egui::Color32::from(theme.accent_primary()));
        } else if resp.hovered() {
            ui.painter()
                .rect_filled(tab_rect, 0.0, theme.overlay_hover().to_egui_premultiplied());
        }

        let fg = if *active {
            theme.text_primary()
        } else {
            theme.text_muted()
        };
        let mut cx = tab_rect.min.x + pad_x;

        let fr = egui::Rect::from_min_size(
            egui::pos2(cx, tab_rect.center().y - icon_xs / 2.0),
            egui::vec2(icon_xs, icon_xs),
        );
        paint_glyph(
            ui,
            FOLDER,
            fr,
            icon_xs,
            egui::Color32::from(theme.text_muted()),
        );
        cx += icon_xs + gap;

        ui.painter().text(
            egui::pos2(cx, tab_rect.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            font.clone(),
            egui::Color32::from(fg),
        );

        // close ✕ — 활성 탭은 상시, 비활성은 hover 시.
        if *active || resp.hovered() {
            let close_rect = egui::Rect::from_min_size(
                egui::pos2(
                    tab_rect.max.x - pad_x - icon_xs,
                    tab_rect.center().y - icon_xs / 2.0,
                ),
                egui::vec2(icon_xs, icon_xs),
            );
            paint_glyph(
                ui,
                CLOSE,
                close_rect,
                icon_xs,
                egui::Color32::from(theme.text_muted()),
            );
        }

        x += tab_w;
    }

    let plus_rect = egui::Rect::from_min_size(egui::pos2(x, rect.min.y), egui::vec2(bar_h, bar_h));
    let plus_resp = ui.interact(
        plus_rect,
        ui.id().with("explorer_tab_new"),
        egui::Sense::click(),
    );
    if plus_resp.hovered() {
        ui.painter().rect_filled(
            plus_rect,
            0.0,
            theme.overlay_hover().to_egui_premultiplied(),
        );
    }
    let icon = theme.icon_glyph_size_md.value();
    let icon_rect = egui::Rect::from_center_size(plus_rect.center(), egui::vec2(icon, icon));
    paint_glyph(
        ui,
        PLUS,
        icon_rect,
        icon,
        egui::Color32::from(theme.text_secondary()),
    );
}

fn paint_glyph(ui: &mut egui::Ui, g: MockGlyph, rect: egui::Rect, size: f32, color: egui::Color32) {
    g.image(size, color).paint_at(ui, rect);
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
            ("width", "fit label (variable)"),
            ("strip", "bg-sidebar + bottom border-strong"),
            ("active", "bg-panel + top 2px accent indicator"),
            ("icon", "folder (text-muted) before label"),
            ("close", "✕ active always · idle on hover"),
            ("controls", "＋ new tab (end)"),
        ],
        &[
            TokenChip::new("bg-sidebar", "strip fill", theme.bg_sidebar().into()),
            TokenChip::new("bg-panel", "active tab", theme.bg_panel().into()),
            TokenChip::new(
                "accent-primary",
                "active indicator",
                theme.accent_primary().into(),
            ),
            TokenChip::new("separator", "tab divider", theme.separator.into()),
        ],
    );

    spec::note(
        ui,
        theme,
        "Surface-local tabs use a top accent indicator and a folder icon before each label. \
         The host saves and restores the internal tab list with the surface.",
    );
}
