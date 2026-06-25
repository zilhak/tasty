//! `Input` primitive specimen — 디자인(4) `components/forms/Input` 카드.
//!
//! default · icon(search) · addon · mono · invalid · disabled · block. focus 시
//! ring(즉시). 상태는 egui memory 에 남도록 specimen 마다 독립 버퍼를 thread_local
//! 로 보관. 하단 `meta` 로 치수/토큰 노출.

use std::cell::RefCell;

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::Input;

use super::glyph;
use crate::catalog::spec::{StageVariant, TokenChip, cluster, meta, stage};

thread_local! {
    static BUFS: RefCell<[String; 7]> = const {
        RefCell::new([
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
        ])
    };
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    let xs = theme.field_width_xs.value();
    let md = theme.field_width_md.value();
    let lg = theme.measure_sm.value();

    BUFS.with(|b| {
        let mut bufs = b.borrow_mut();
        stage(ui, theme, StageVariant::Column, |ui| {
            cluster(ui, theme, "default · icon(search) · addon — click to focus", |ui| {
                Input::new()
                    .placeholder("Workspace name")
                    .width(md)
                    .show(ui, theme, &mut bufs[0]);
                Input::new()
                    .placeholder("Filter…")
                    .width(md)
                    .icon(&|ui, rect, c| glyph::SEARCH.image(rect.height(), c).paint_at(ui, rect))
                    .show(ui, theme, &mut bufs[1]);
                Input::new()
                    .mono(true)
                    .addon("px")
                    .width(xs)
                    .show(ui, theme, &mut bufs[2]);
            });
            cluster(ui, theme, "mono · invalid · disabled", |ui| {
                Input::new()
                    .mono(true)
                    .placeholder("s_01HXK9")
                    .width(md)
                    .show(ui, theme, &mut bufs[3]);
                Input::new()
                    .invalid(true)
                    .placeholder("bad value")
                    .width(md)
                    .show(ui, theme, &mut bufs[4]);
                Input::new()
                    .enabled(false)
                    .placeholder("Disabled")
                    .width(md)
                    .show(ui, theme, &mut bufs[5]);
            });
            cluster(ui, theme, "block — fill container width", |ui| {
                ui.vertical(|ui| {
                    ui.set_max_width(lg);
                    Input::new()
                        .placeholder("Type to search commands…")
                        .icon(&|ui, rect, c| {
                            glyph::SEARCH.image(rect.height(), c).paint_at(ui, rect)
                        })
                        .show(ui, theme, &mut bufs[6]);
                });
            });
        });
    });

    meta(
        ui,
        theme,
        &[
            ("height", "28 control-height"),
            ("border", "1px → focus 2px"),
            ("padding", "0 space-sm"),
        ],
        &[
            TokenChip::new(
                "surface-raised",
                "field fill",
                egui::Color32::from(theme.surface_raised()),
            ),
            TokenChip::new(
                "border-focus",
                "focus ring",
                egui::Color32::from(theme.border_focus()),
            ),
            TokenChip::new(
                "accent-danger",
                "invalid edge",
                egui::Color32::from(theme.accent_danger()),
            ),
            TokenChip::new(
                "text-placeholder",
                "placeholder",
                egui::Color32::from(theme.text_placeholder()),
            ),
        ],
    );
}
