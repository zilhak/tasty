//! `explorer_toolbar` specimen — 디자인 T11 explorer 툴바의 편집형 주소표시줄(`PathField`) +
//! view-mode 아이콘 토글 (design `ExpToolbar` / `PathField` / `SegToggle`).
//!
//! - **주소표시줄**(design `PathField`): 공용 편집형 경로 필드 — folderOpen leading + mono 경로
//!   (idle=secondary / editing=primary) + Go(arrow-right). 클릭→편집, 경로 타이핑 후 `↵`/Go 로
//!   디렉토리 이동. 후보(최근 디렉토리) 드롭다운 = AutoComplete. breadcrumb 는 폐기.
//! - **SegToggle**: 컨테이너 surface-raised + border-default + radius, grid/list/detail
//!   아이콘 세그먼트. active = surface-active bg + text-primary, inactive = text-muted.
//!
//! 색·치수·폰트는 전부 `Theme` 토큰. 본체 `explorer.rs` 의 `address_bar`(PathField)/`seg_toggle`
//! 와 동일 형상(구조 전사).

use std::cell::RefCell;

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::PathField;

use super::glyph;
use crate::catalog::icons::{LAYOUT_DETAIL, LAYOUT_GRID, LIST, MockGlyph};
use crate::catalog::spec::{StageVariant, TokenChip, cluster, meta, note, stage};

/// 최근 디렉토리 후보(design ExpToolbar `PathField candidates`).
const EXP_RECENT: &[&str] = &[
    "~/Downloads",
    "~/work/tasty",
    "~/work/tasty/crates/tasty-ui-widgets",
    "~/.config/tasty",
];

/// 라이브 PathField 편집 상태(호출측 소유 계약).
struct AddrState {
    buf: String,
    editing: bool,
    active: Option<usize>,
}

thread_local! {
    static SEG_SEL: RefCell<usize> = const { RefCell::new(2) }; // detail 기본 활성
    static ADDR: RefCell<Option<AddrState>> = const { RefCell::new(None) };
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    let folder_icon = |ui: &mut egui::Ui, rect: egui::Rect, c: egui::Color32| {
        glyph::FOLDER_OPEN
            .image(rect.height(), c)
            .paint_at(ui, rect);
    };
    let go_icon = |ui: &mut egui::Ui, rect: egui::Rect, c: egui::Color32| {
        glyph::ARROW_RIGHT
            .image(rect.height(), c)
            .paint_at(ui, rect);
    };

    // ── 편집형 주소표시줄 (PathField) ──
    stage(ui, theme, StageVariant::Tight, |ui| {
        egui::Frame::new()
            .fill(egui::Color32::from(theme.bg_panel()))
            .inner_margin(egui::Margin::same(theme.spacing_sm.value() as i8))
            .show(ui, |ui| {
                ui.set_width(theme.measure_md.value());
                ADDR.with(|s| {
                    let mut slot = s.borrow_mut();
                    let st = slot.get_or_insert_with(|| AddrState {
                        buf: "~/Downloads".to_string(),
                        editing: false,
                        active: None,
                    });
                    PathField::new("gallery_exp_addr")
                        .placeholder("Go to directory…")
                        .empty_label("No matching path")
                        .leading_icon(&folder_icon)
                        .row_icon(&folder_icon)
                        .go_icon(&go_icon)
                        .show(
                            ui,
                            theme,
                            &mut st.buf,
                            &mut st.editing,
                            &mut st.active,
                            EXP_RECENT,
                            "~/Downloads",
                        );
                });
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
            ("field", "PathField — input-bg + input-border(-focus)"),
            ("leading", "folderOpen (input-icon-fg)"),
            ("trailing", "Go IconButton (sm) — arrow-right"),
            ("keys", "Enter/Go navigate · ↑/↓ active · Esc revert"),
            ("toggle", "3 icons · active surface-active"),
            ("height", "28 (control-height-interactive)"),
        ],
        &[
            TokenChip::new(
                "input-bg",
                "field fill",
                egui::Color32::from(theme.surface_raised()),
            ),
            TokenChip::new(
                "text-secondary",
                "idle path",
                egui::Color32::from(theme.text_secondary()),
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
        "The address bar is now the shared editable PathField (folderOpen + mono path + Go), \
         replacing the old breadcrumb — click to edit, type a directory and press Enter / Go \
         to move cwd; recent directories surface in the AutoComplete dropdown. Mirrors the main \
         app's address_bar (PathField) / seg_toggle. The toggle uses grid / list / detail icons \
         — active segment fills surface-active with text-primary, inactive stays text-muted.",
    );
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
