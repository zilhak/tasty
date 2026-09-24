//! 공용 ListCtrl의 선택·비활성 행 예제. 항목별 설명과 Active 태그를 함께 표시한다.

use std::cell::RefCell;

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{ListCtrl, ListCtrlItem, TagVariant, tag};

use crate::catalog::spec::{StageVariant, TokenChip, meta, stage};

thread_local! {
    static SEL: RefCell<usize> = const { RefCell::new(0) };
}

/// ListCtrl — label · description · trailing Tag · chevron · selected · disabled.
pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    stage(ui, theme, StageVariant::Tight, |ui| {
        egui::Frame::new()
            .fill(egui::Color32::from(theme.bg_panel()))
            .stroke(egui::Stroke::new(
                theme.border_width.value(),
                egui::Color32::from(theme.border_default()),
            ))
            .corner_radius(theme.corner_radius.value())
            .show(ui, |ui| {
                ui.set_width(theme.measure_md.value());
                SEL.with(|s| {
                    let mut sel = s.borrow_mut();
                    let active_tag = |ui: &mut egui::Ui, th: &Theme| {
                        tag(ui, th, "Active", TagVariant::Success, true);
                    };
                    let items = [
                        ListCtrlItem::new("Default")
                            .description("Tasty stock bindings")
                            .trailing(&active_tag),
                        ListCtrlItem::new("Mac").description("⌘-based, TextEdit-style"),
                        ListCtrlItem::new("Vim").description("modal, hjkl motions"),
                        ListCtrlItem::new("Custom").disabled(true),
                    ];
                    let out = ListCtrl::new().show(ui, theme, &items, Some(*sel));
                    if let Some(i) = out.clicked {
                        *sel = i;
                    }
                });
            });
    });

    meta(
        ui,
        theme,
        &[
            ("row", "min-height 36 + desc"),
            ("selected", "surface-active + 2px bar"),
            ("divided", "separator hairline"),
        ],
        &[
            TokenChip::new(
                "surface-active",
                "selected row",
                egui::Color32::from(theme.listctrl_row_bg_selected()),
            ),
            TokenChip::new(
                "accent-primary",
                "selected left bar",
                egui::Color32::from(theme.listctrl_selected_bar()),
            ),
            TokenChip::new(
                "overlay-hover",
                "hover row",
                egui::Color32::from(theme.listctrl_row_bg_hover()),
            ),
            TokenChip::new(
                "text-muted",
                "desc · chevron",
                egui::Color32::from(theme.listctrl_desc_fg()),
            ),
        ],
    );
}
