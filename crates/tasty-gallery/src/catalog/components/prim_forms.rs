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
        RefCell::new(FormState {
            sel: 0,
            sel_long: 3,
            check_a: true,
            check_b: false,
            switch_a: true,
            switch_b: false,
        })
    };
}

struct FormState {
    sel: usize,
    sel_long: usize,
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
            cluster(ui, theme, "Select", |ui| {
                let opts = ["Default (full rc)", "Tasty rc", "Custom"];
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
            // 회귀 방지: field_width_md(160px) 가용 폭(~108px)을 넘는 긴 옵션 라벨 —
            // Codex 플러그인 default_approval_policy select 재현 케이스.
            cluster(ui, theme, "Select (long text)", |ui| {
                let opts = [
                    "상속 (codex 기본값)",
                    "Untrusted (신뢰되지 않은 명령만 승인 요청)",
                    "On request (모델이 판단)",
                    "Never (승인 프롬프트 없음)",
                ];
                select(
                    ui,
                    theme,
                    "gallery_select_long",
                    &mut st.sel_long,
                    &opts,
                    field_md,
                    true,
                );
            });
            cluster(ui, theme, "Checkbox", |ui| {
                checkbox(ui, theme, &mut st.check_a, "Confirm on close", true);
                checkbox(ui, theme, &mut st.check_b, "Restore layout", true);
            });
            cluster(ui, theme, "Switch", |ui| {
                switch(ui, theme, &mut st.switch_a, Some("Ligatures"), true);
                switch(ui, theme, &mut st.switch_b, Some("Reduced motion"), true);
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
