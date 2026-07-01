//! `explorer_toolbar` specimen — 디자인 T11 explorer 툴바의 주소표시줄 박스 +
//! view-mode 아이콘 토글 (design `ExpToolbar` / `Crumb` / `SegToggle`).
//!
//! - **주소표시줄**(design address bar): surface-raised 배경 + border-default 1px +
//!   radius 박스, 앞에 `folderOpen` 아이콘(text-muted), 크럼 사이 `chevRsm` 아이콘.
//! - **SegToggle**: 컨테이너 surface-raised + border-default + radius, grid/list/detail
//!   아이콘 세그먼트. active = surface-active bg + text-primary, inactive = text-muted.
//!
//! 색·치수·폰트는 전부 `Theme` 토큰. 본체 `explorer.rs` 의 `address_bar`/`seg_toggle`
//! 와 동일 형상(구조 전사).

use std::cell::RefCell;

use tasty_type_appearance::theme::Theme;

use crate::catalog::icons::{
    CHEVRON_RIGHT, FOLDER_OPEN, LAYOUT_DETAIL, LAYOUT_GRID, LIST, MockGlyph,
};
use crate::catalog::spec::{StageVariant, TokenChip, cluster, meta, note, stage};

const CRUMBS: &[&str] = &["Home", "user", "Downloads"];

thread_local! {
    static SEG_SEL: RefCell<usize> = const { RefCell::new(2) }; // detail 기본 활성
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    // ── 주소표시줄 박스 ──
    stage(ui, theme, StageVariant::Tight, |ui| {
        egui::Frame::new()
            .fill(egui::Color32::from(theme.bg_panel()))
            .inner_margin(egui::Margin::same(theme.spacing_sm.value() as i8))
            .show(ui, |ui| {
                ui.set_width(theme.measure_md.value());
                address_bar(ui, theme, CRUMBS);
            });
    });

    // ── view-mode 아이콘 토글 ──
    cluster(ui, theme, "view-mode toggle (grid / list / detail)", |ui| {
        egui::Frame::new()
            .fill(egui::Color32::from(theme.bg_panel()))
            .inner_margin(egui::Margin::same(theme.spacing_sm.value() as i8))
            .show(ui, |ui| {
                SEG_SEL.with(|s| {
                    let mut sel = s.borrow_mut();
                    if let Some(i) = seg_toggle(ui, theme, *sel) {
                        *sel = i;
                    }
                });
            });
    });

    meta(
        ui,
        theme,
        &[
            ("address bar", "surface-raised + border-default + radius"),
            ("crumb sep", "chevron (text-muted · 0.7)"),
            ("toggle", "3 icons · active surface-active"),
            ("height", "28 (control-height-interactive)"),
        ],
        &[
            TokenChip::new(
                "surface-raised",
                "box / toggle bg",
                egui::Color32::from(theme.surface_raised()),
            ),
            TokenChip::new(
                "border-default",
                "1px border",
                egui::Color32::from(theme.border_default()),
            ),
            TokenChip::new(
                "surface-active",
                "active segment",
                egui::Color32::from(theme.surface_active()),
            ),
            TokenChip::new(
                "text-muted",
                "folder / inactive",
                egui::Color32::from(theme.text_muted()),
            ),
        ],
    );

    note(
        ui,
        theme,
        "Address bar clips its crumbs to the box width so a long path never overflows into \
         the view-mode toggle (design flex address:1 / toggle:none). The toggle uses grid / \
         list / detail icons — active segment fills surface-active with text-primary, inactive \
         stays text-muted. Mirrors the main app's address_bar / seg_toggle.",
    );
}

/// 주소표시줄 박스 한 개. 내용은 박스 폭으로 clip.
fn address_bar(ui: &mut egui::Ui, theme: &Theme, crumbs: &[&str]) {
    let h = theme.item_height_interactive.value();
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
    ui.painter().rect(
        rect,
        theme.corner_radius.value(),
        egui::Color32::from(theme.surface_raised()),
        egui::Stroke::new(
            theme.border_width.value(),
            egui::Color32::from(theme.border_default()),
        ),
        egui::StrokeKind::Inside,
    );
    let pad = theme.spacing_xs.value();
    let inner = rect.shrink2(egui::vec2(pad + theme.border_width.value(), 0.0));
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    child.set_clip_rect(inner);
    child.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);

    let icon_sm = theme.icon_glyph_size_sm.value();
    let (fi, _) = child.allocate_exact_size(egui::vec2(icon_sm, icon_sm), egui::Sense::hover());
    FOLDER_OPEN
        .image(icon_sm, egui::Color32::from(theme.text_muted()))
        .paint_at(&child, fi);
    child.add_space(theme.spacing_xs.value());

    let font = egui::FontId::proportional(theme.font_size_body.value());
    let last = crumbs.len().saturating_sub(1);
    for (i, name) in crumbs.iter().enumerate() {
        let is_last = i == last;
        let galley = child
            .fonts(|f| f.layout_no_wrap((*name).to_string(), font.clone(), egui::Color32::WHITE));
        let crumb_pad = theme.spacing_xs.value();
        let (crect, _) = child.allocate_exact_size(
            egui::vec2(galley.size().x + crumb_pad * 2.0, h),
            egui::Sense::hover(),
        );
        let color = if is_last {
            egui::Color32::from(theme.text_primary())
        } else {
            egui::Color32::from(theme.text_secondary())
        };
        child.painter().text(
            egui::pos2(crect.min.x + crumb_pad, crect.center().y),
            egui::Align2::LEFT_CENTER,
            name,
            font.clone(),
            color,
        );
        if !is_last {
            let (srect, _) =
                child.allocate_exact_size(egui::vec2(icon_sm, h), egui::Sense::hover());
            let xs = theme.icon_glyph_size_xs.value();
            let sr = egui::Rect::from_center_size(srect.center(), egui::vec2(xs, xs));
            CHEVRON_RIGHT
                .image(
                    xs,
                    egui::Color32::from(theme.text_muted()).gamma_multiply(0.7),
                )
                .paint_at(&child, sr);
        }
    }
}

/// grid/list/detail 아이콘 토글. 클릭된 세그먼트 index 반환(없으면 None).
fn seg_toggle(ui: &mut egui::Ui, theme: &Theme, selected: usize) -> Option<usize> {
    let pad = theme.spacing_xs.value();
    let gap = theme.spacing_xs.value();
    let h = theme.item_height_interactive.value();
    let seg_w = theme.icon_glyph_size_md.value() + theme.spacing_sm.value();
    let icon = theme.icon_glyph_size_md.value();
    let total_w = pad * 2.0 + seg_w * 3.0 + gap * 2.0 + theme.border_width.value() * 2.0;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(total_w, h), egui::Sense::hover());
    ui.painter().rect(
        rect,
        theme.corner_radius.value(),
        egui::Color32::from(theme.surface_raised()),
        egui::Stroke::new(
            theme.border_width.value(),
            egui::Color32::from(theme.border_default()),
        ),
        egui::StrokeKind::Inside,
    );
    let glyphs: [MockGlyph; 3] = [LAYOUT_GRID, LIST, LAYOUT_DETAIL];
    let seg_h = h - pad * 2.0;
    let mut sx = rect.min.x + theme.border_width.value() + pad;
    let mut clicked = None;
    for (i, g) in glyphs.into_iter().enumerate() {
        let seg_rect = egui::Rect::from_min_size(
            egui::pos2(sx, rect.center().y - seg_h / 2.0),
            egui::vec2(seg_w, seg_h),
        );
        let resp = ui.interact(seg_rect, ui.id().with(("gal_seg", i)), egui::Sense::click());
        let active = i == selected;
        if active {
            ui.painter().rect_filled(
                seg_rect,
                theme.corner_radius_sm.value(),
                egui::Color32::from(theme.surface_active()),
            );
        } else if resp.hovered() {
            ui.painter().rect_filled(
                seg_rect,
                theme.corner_radius_sm.value(),
                theme.overlay_hover().to_egui_premultiplied(),
            );
        }
        let fg = if active {
            egui::Color32::from(theme.text_primary())
        } else {
            egui::Color32::from(theme.text_muted())
        };
        let ir = egui::Rect::from_center_size(seg_rect.center(), egui::vec2(icon, icon));
        g.image(icon, fg).paint_at(ui, ir);
        if resp.clicked() {
            clicked = Some(i);
        }
        sx += seg_w + gap;
    }
    clicked
}
