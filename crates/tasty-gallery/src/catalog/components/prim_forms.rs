//! Select · Checkbox · Switch primitive specimen — 디자인(4) `components/forms` 카드.
//!
//! Select(토큰 트리거 + 드롭다운) · Checkbox(16px square) · Switch(28×16 track).
//! 상태는 thread_local 로 보관. 하단 `meta` 로 치수/토큰 노출.

use std::cell::RefCell;

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{checkbox, select, switch};

use crate::catalog::spec::{StageVariant, TokenChip, cluster, meta, stage};

thread_local! {
    static STATE: RefCell<FormState> = const {
        RefCell::new(FormState { sel: 0, check_a: true, check_b: false, switch_a: true, switch_b: false })
    };
}

struct FormState {
    sel: usize,
    check_a: bool,
    check_b: bool,
    switch_a: bool,
    switch_b: bool,
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    let field_md = theme.field_width_md.value();
    STATE.with(|s| {
        let mut st = s.borrow_mut();
        stage(ui, theme, StageVariant::Column, |ui| {
            cluster(ui, theme, "Select — token trigger + dropdown", |ui| {
                let opts = ["Mocha (dark)", "Latte (light)", "Auto"];
                select(
                    ui,
                    theme,
                    "gallery_theme",
                    &mut st.sel,
                    &opts,
                    field_md,
                    true,
                );
            });
            cluster(
                ui,
                theme,
                "Checkbox — checked · unchecked · disabled",
                |ui| {
                    checkbox(ui, theme, &mut st.check_a, "Restore layout", true);
                    checkbox(ui, theme, &mut st.check_b, "Confirm on close", true);
                    let mut off = false;
                    checkbox(ui, theme, &mut off, "Disabled", false);
                },
            );
            cluster(ui, theme, "Switch — on · off · disabled", |ui| {
                switch(ui, theme, &mut st.switch_a, Some("Reduced motion"), true);
                switch(ui, theme, &mut st.switch_b, Some("High contrast"), true);
                let mut off = false;
                switch(ui, theme, &mut off, Some("Disabled"), false);
            });
        });
    });

    meta(
        ui,
        theme,
        &[
            ("height", "28 control-height"),
            ("checkbox", "16px square"),
            ("switch", "28×16 track"),
            ("accent", "primary"),
        ],
        &[
            TokenChip::new(
                "accent-primary",
                "checked fill",
                egui::Color32::from(theme.accent_primary()),
            ),
            TokenChip::new(
                "surface-raised",
                "control fill",
                egui::Color32::from(theme.surface_raised()),
            ),
            TokenChip::new(
                "border-default",
                "control edge",
                egui::Color32::from(theme.border_default()),
            ),
        ],
    );
}
