//! `sidebar` specimen — Sidebar & rail (research §2.5 Layouts).
//!
//! 좌측 네비게이션. 두 폭:
//! - **Full 212**: 로고+워드마크 헤더 / "Workspaces" railHead / 워크스페이스 행
//!   (dot + name + badge, 활성행 surface-active + 2px inset accent) / footer
//!   (Tools·Plugins·Settings ghost 블록, 상단 border).
//! - **Collapsed rail 52**: 로고 24 + IconButton 28 슬롯들.
//!
//! Theme 토큰만으로 정적 재현 (binary 미의존).

use tasty_type_appearance::theme::Theme;

use crate::catalog::icons::{FOLDER, MockGlyph, PLUG, SETTINGS, TERMINAL};
use crate::catalog::spec::{self, StageVariant, TokenChip};

/// (name, badge, active)
const WORKSPACES: &[(&str, Option<&str>, bool)] =
    &[("main", None, true), ("review", Some("3"), false), ("agent", None, false)];
/// footer ghost rows.
const FOOTER: &[(MockGlyph, &str)] =
    &[(TERMINAL, "Tools"), (PLUG, "Plugins"), (SETTINGS, "Settings")];
/// collapsed rail slots.
const RAIL_SLOTS: &[MockGlyph] = &[TERMINAL, FOLDER, PLUG, SETTINGS];

fn paint_icon(
    ui: &mut egui::Ui,
    glyph: MockGlyph,
    center: egui::Pos2,
    size: f32,
    color: egui::Color32,
) {
    let r = egui::Rect::from_center_size(center, egui::vec2(size, size));
    glyph.image(size, color).paint_at(ui, r);
}

fn full(ui: &mut egui::Ui, theme: &Theme) {
    let w = theme.field_width_lg.value() + theme.spacing_md.value(); // 212
    let h = theme.spacing_xl.value() * 15.0; // 360
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, theme.corner_radius.value(), egui::Color32::from(theme.bg_sidebar()));

    let pad = theme.spacing_md.value(); // 12
    let row_h = theme.item_height_interactive.value(); // 28
    let mut y = rect.min.y + pad;

    // ── header: logo + wordmark ──
    let logo = theme.sidebar_logo_size.value(); // 22
    let logo_c = egui::pos2(rect.min.x + pad + logo * 0.5, y + logo * 0.5);
    paint_icon(ui, TERMINAL, logo_c, logo, egui::Color32::from(theme.accent_primary()));
    p.text(
        egui::pos2(logo_c.x + logo * 0.5 + theme.spacing_sm.value(), logo_c.y),
        egui::Align2::LEFT_CENTER,
        "Tasty",
        egui::FontId::proportional(theme.sidebar_wordmark_font_size.value()),
        egui::Color32::from(theme.text_primary()),
    );
    y += logo + theme.spacing_xs.value() + theme.spacing_md.value();

    // ── railHead: "WORKSPACES" ──
    p.text(
        egui::pos2(rect.min.x + pad, y),
        egui::Align2::LEFT_TOP,
        "WORKSPACES",
        egui::FontId::proportional(theme.sidebar_section_heading_font_size.value()),
        egui::Color32::from(theme.text_muted()),
    );
    y += theme.spacing_lg.value();

    // ── workspace rows ──
    for (name, badge, active) in WORKSPACES {
        let row = egui::Rect::from_min_size(
            egui::pos2(rect.min.x + theme.spacing_xs.value(), y),
            egui::vec2(w - theme.spacing_xs.value() * 2.0, row_h),
        );
        if *active {
            p.rect_filled(
                row,
                theme.corner_radius_sm.value(),
                egui::Color32::from(theme.surface_active()),
            );
            // 2px inset accent bar.
            let bar = egui::Rect::from_min_size(
                row.min,
                egui::vec2(theme.focus_ring_width.value(), row.height()),
            );
            p.rect_filled(bar, 0.0, egui::Color32::from(theme.accent_primary()));
        }
        let dot_r = theme.status_dot_size.value() * 0.5;
        let dc = egui::pos2(row.min.x + theme.spacing_md.value() + dot_r, row.center().y);
        p.circle_filled(
            dc,
            dot_r,
            egui::Color32::from(if *active {
                theme.accent_success()
            } else {
                theme.text_muted()
            }),
        );
        p.text(
            egui::pos2(dc.x + dot_r + theme.spacing_sm.value(), row.center().y),
            egui::Align2::LEFT_CENTER,
            name,
            egui::FontId::proportional(theme.font_size_body.value()),
            egui::Color32::from(if *active {
                theme.text_primary()
            } else {
                theme.text_secondary()
            }),
        );
        if let Some(b) = badge {
            let bw = theme.spacing_lg.value();
            let badge_rect = egui::Rect::from_min_size(
                egui::pos2(
                    row.max.x - theme.spacing_sm.value() - bw,
                    row.center().y - theme.spacing_sm.value(),
                ),
                egui::vec2(bw, theme.spacing_lg.value()),
            );
            p.rect_filled(
                badge_rect,
                theme.corner_radius_sm.value(),
                egui::Color32::from(theme.surface_raised()),
            );
            p.text(
                badge_rect.center(),
                egui::Align2::CENTER_CENTER,
                *b,
                egui::FontId::proportional(theme.font_size_micro.value()),
                egui::Color32::from(theme.text_secondary()),
            );
        }
        y += row_h + theme.spacing_xs.value();
    }

    // ── footer: border-top + ghost rows (bottom-anchored) ──
    let footer_h = row_h * FOOTER.len() as f32 + pad;
    let footer_top = rect.max.y - footer_h;
    p.hline(
        rect.x_range(),
        footer_top,
        egui::Stroke::new(
            theme.border_width.value(),
            egui::Color32::from(theme.border_default()),
        ),
    );
    let mut fy = footer_top + theme.spacing_sm.value();
    for (glyph, label) in FOOTER {
        let cy = fy + row_h * 0.5;
        paint_icon(
            ui,
            *glyph,
            egui::pos2(rect.min.x + pad + theme.icon_glyph_size_sm.value() * 0.5, cy),
            theme.icon_glyph_size_sm.value(),
            egui::Color32::from(theme.text_muted()),
        );
        p.text(
            egui::pos2(
                rect.min.x + pad + theme.icon_glyph_size_sm.value() + theme.spacing_sm.value(),
                cy,
            ),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(theme.sidebar_button_label_font_size.value()),
            egui::Color32::from(theme.text_secondary()),
        );
        fy += row_h;
    }
}

fn rail(ui: &mut egui::Ui, theme: &Theme) {
    // 52 = collapsed slot 32 + lg 16 + xs 4.
    let w = theme.sidebar_collapsed_slot_width.value()
        + theme.spacing_lg.value()
        + theme.spacing_xs.value();
    let h = theme.spacing_xl.value() * 15.0; // 360
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, theme.corner_radius.value(), egui::Color32::from(theme.bg_sidebar()));

    let cx = rect.center().x;
    let mut y = rect.min.y + theme.spacing_md.value();

    // 로고 24.
    let logo = theme.sidebar_logo_collapsed_size.value(); // 24
    paint_icon(
        ui,
        TERMINAL,
        egui::pos2(cx, y + logo * 0.5),
        logo,
        egui::Color32::from(theme.accent_primary()),
    );
    y += logo + theme.spacing_md.value();

    // IconButton 28 슬롯들.
    let slot = theme.item_height_interactive.value(); // 28
    for (i, glyph) in RAIL_SLOTS.iter().enumerate() {
        let area =
            egui::Rect::from_center_size(egui::pos2(cx, y + slot * 0.5), egui::vec2(slot, slot));
        if i == 0 {
            p.rect_filled(
                area,
                theme.corner_radius_sm.value(),
                egui::Color32::from(theme.surface_active()),
            );
        }
        paint_icon(
            ui,
            *glyph,
            area.center(),
            theme.icon_glyph_size_md.value(),
            egui::Color32::from(if i == 0 {
                theme.text_primary()
            } else {
                theme.text_muted()
            }),
        );
        y += slot + theme.spacing_sm.value();
    }
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        spec::cluster(ui, theme, "Full · 212", |ui| full(ui, theme));
        spec::cluster(ui, theme, "Collapsed rail · 52", |ui| rail(ui, theme));
    });

    spec::meta(
        ui,
        theme,
        &[
            ("full width", "212"),
            ("rail width", "52"),
            ("logo", "22 full / 24 rail"),
            ("row", "dot + name + badge"),
            ("active row", "surface-active + 2px inset accent"),
            ("footer", "Tools·Plugins·Settings, border-top"),
        ],
        &[
            TokenChip::new("bg-sidebar", "sidebar fill", theme.bg_sidebar().into()),
            TokenChip::new("surface-active", "active row", theme.surface_active().into()),
            TokenChip::new("accent-primary", "inset bar + logo", theme.accent_primary().into()),
            TokenChip::new("border-default", "footer divider", theme.border_default().into()),
        ],
    );

    spec::note(
        ui,
        theme,
        "Full 은 워크스페이스를 이름·badge 까지 펼치고, 접으면 52px rail 로 줄어 \
         아이콘 슬롯만 남는다. 활성 행은 surface-active + 좌측 2px accent 로 표시.",
    );
}
