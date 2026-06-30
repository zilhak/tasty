//! `segmented` specimen — 디자인 T11 explorer view-mode 세그먼트 토글 (design §3.4).
//!
//! 본체와 **동일한** `tasty_ui_widgets::segmented` 공용 위젯을 호출(demo=main).
//! explorer 툴바의 grid/list/detail 전환이 1차 사용처. 활성 세그먼트는
//! accent-primary fill + text-on-accent, 비활성은 text-secondary(+hover overlay).
//!
//! i18n 키 후보(본체 반영 시): `explorer.view.grid/list/detail`.

use std::cell::RefCell;

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::segmented;

use crate::catalog::spec::{StageVariant, TokenChip, cluster, meta, note, stage};

thread_local! {
    static VIEW_SEL: RefCell<usize> = const { RefCell::new(0) };
    static SEG2_SEL: RefCell<usize> = const { RefCell::new(1) };
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    stage(ui, theme, StageVariant::Column, |ui| {
        cluster(
            ui,
            theme,
            "view mode — grid · list · detail (click them)",
            |ui| {
                VIEW_SEL.with(|s| {
                    let mut sel = s.borrow_mut();
                    if let Some(i) = segmented(ui, theme, &["grid", "list", "detail"], *sel) {
                        *sel = i;
                    }
                });
            },
        );
        cluster(ui, theme, "two segments", |ui| {
            SEG2_SEL.with(|s| {
                let mut sel = s.borrow_mut();
                if let Some(i) = segmented(ui, theme, &["Edit", "Preview"], *sel) {
                    *sel = i;
                }
            });
        });
    });

    meta(
        ui,
        theme,
        &[
            ("height", "28 control-height"),
            ("segment pad", "0 space-sm"),
            ("container", "surface-raised · 1px border-strong"),
            ("active", "accent-primary fill · text-on-accent"),
            ("divider", "1px separator (inactive edges)"),
        ],
        &[
            TokenChip::new(
                "surface-raised",
                "container",
                egui::Color32::from(theme.surface_raised()),
            ),
            TokenChip::new(
                "accent-primary",
                "active fill",
                egui::Color32::from(theme.accent_primary()),
            ),
            TokenChip::new(
                "text-on-accent",
                "active label",
                egui::Color32::from(theme.text_on_accent()),
            ),
            TokenChip::new(
                "text-secondary",
                "idle label",
                egui::Color32::from(theme.text_secondary()),
            ),
        ],
    );

    note(
        ui,
        theme,
        "General-purpose mutually-exclusive toggle — explorer's grid/list/detail switch \
         is the first caller. The active segment lifts on accent fill, not just colored text.",
    );
}
