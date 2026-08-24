//! Select · Multi-select · Checkbox · Switch primitive specimen — 디자인(4)
//! `components/forms` 카드.
//!
//! Select(토큰 트리거 + 드롭다운) · Multi-select(같은 트리거 + checkbox 행 팝업) ·
//! Checkbox(16px square) · Switch(28×16 track). 상태는 thread_local 로 보관.
//! 하단 `meta` 로 치수/토큰 노출.

use std::cell::RefCell;

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{MultiSelectLabels, checkbox, multi_select, select, switch};

use crate::catalog::spec::{StageVariant, TokenChip, cluster, meta, stage};

thread_local! {
    static STATE: RefCell<FormState> = const {
        RefCell::new(FormState {
            sel: 0,
            sel_long: 3,
            multi: [true, true, true, false, false],
            multi_long: [false, true, false, false],
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
    /// Multi-select 데모 — DAG 상태 필터를 본뜬 5종.
    multi: [bool; 5],
    /// 긴 라벨 회귀 케이스용 4종.
    multi_long: [bool; 4],
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
            // 다중선택 — 트리거는 위 Select 와 같은 토큰이고, 팝업만 checkbox 행이다.
            // 나란히 놓아 높이·보더·폰트가 같은 계열로 읽히는지 눈으로 대조한다.
            cluster(ui, theme, "Multi-select", |ui| {
                let opts = ["Waiting", "Ready", "Running", "Done", "Failed"];
                // 문구는 위젯이 아니라 호출자가 주입한다(위젯 crate 는 i18n 미의존).
                // 갤러리는 본체가 아니라 specimen 이라 영어 리터럴을 그대로 쓴다.
                let labels = MultiSelectLabels {
                    none: "No status",
                    some: "{} selected",
                    all: "All statuses",
                };
                multi_select(
                    ui,
                    theme,
                    "gallery_multi_status",
                    &mut st.multi,
                    &opts,
                    &labels,
                    field_md,
                    true,
                );
            });
            // 회귀 방지: 요약 라벨과 팝업 행 라벨 **양쪽**이 가용 폭을 넘는 케이스.
            // 트리거는 말줄임(truncate_at_width), 팝업은 내용만큼 넓어진다.
            cluster(ui, theme, "Multi-select (long text)", |ui| {
                let opts = [
                    "Untrusted (신뢰되지 않은 명령만 승인 요청)",
                    "On request (모델이 판단)",
                    "Never (승인 프롬프트 없음)",
                    "상속 (codex 기본값)",
                ];
                let labels = MultiSelectLabels {
                    none: "승인 정책을 선택하세요",
                    some: "승인 정책 {}개를 선택했습니다",
                    all: "모든 승인 정책을 선택했습니다",
                };
                multi_select(
                    ui,
                    theme,
                    "gallery_multi_long",
                    &mut st.multi_long,
                    &opts,
                    &labels,
                    field_md,
                    true,
                );
            });
            // 비활성 — 트리거만 dim 되고 클릭해도 팝업이 열리지 않는다.
            cluster(ui, theme, "Multi-select (disabled)", |ui| {
                let opts = ["Waiting", "Ready", "Running", "Done", "Failed"];
                let labels = MultiSelectLabels {
                    none: "No status",
                    some: "{} selected",
                    all: "All statuses",
                };
                let mut frozen = st.multi;
                multi_select(
                    ui,
                    theme,
                    "gallery_multi_disabled",
                    &mut frozen,
                    &opts,
                    &labels,
                    field_md,
                    false,
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
            ("multi-select", "select trigger + checkbox rows"),
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
