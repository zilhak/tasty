//! Select, Multi-select, Checkbox, Switch의 상태별 예제.

use std::cell::RefCell;

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{
    MultiSelectAllToggle, MultiSelectLabels, checkbox, multi_select, select, select_or_placeholder,
    switch,
};

use crate::catalog::spec::{StageVariant, TokenChip, cluster, meta, stage};

thread_local! {
    static STATE: RefCell<FormState> = const {
        RefCell::new(FormState {
            sel: 0,
            sel_long: 3,
            sel_placeholder: None,
            multi: [true, true, true, false, false],
            multi_long: [false, true, false, false],
            multi_scroll: [false; 20],
            multi_rows: [false, true, false, false, false],
            multi_all: [true, false, false, true, false, false, false, false, true, false],
            multi_all_masked: [false, true, true, true, true, true, true, true],
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
    /// "아직 안 고름" 상태를 가진 Select — 처음엔 비어 있어 placeholder 가 보인다.
    sel_placeholder: Option<usize>,
    /// Multi-select 데모 — DAG 상태 필터를 본뜬 5종.
    multi: [bool; 5],
    /// 긴 라벨 회귀 케이스용 4종.
    multi_long: [bool; 4],
    /// max-height 스크롤 회귀 케이스용 20종.
    multi_scroll: [bool; 20],
    /// 행 단위 disabled 케이스용 5종 — 마스크는 [`MULTI_ROW_DISABLED`].
    multi_rows: [bool; 5],
    /// 일괄 토글(allToggle) 케이스용 10종 — 옵션 목록은 [`MULTI_ALL_OPTIONS`].
    multi_all: [bool; 10],
    /// 비활성 두 행을 제외한 나머지를 선택해 Clear all 동작을 비교한다.
    multi_all_masked: [bool; 8],
    check_a: bool,
    check_b: bool,
    switch_a: bool,
    switch_b: bool,
}

/// 스크롤 높이와 긴 첫 라벨의 말줄임을 함께 확인할 옵션 목록.
const MULTI_SCROLL_OPTIONS: [&str; 20] = [
    "Very long column label that overflows the menu max width",
    "PID",
    "Port",
    "Protocol",
    "State",
    "Process",
    "Command",
    "User",
    "Started",
    "CPU",
    "Memory",
    "Threads",
    "Handles",
    "Parent",
    "Session",
    "Container",
    "Namespace",
    "Interface",
    "Address",
    "Latency",
];

/// 선택 여부가 다른 두 행을 비활성화해 체크마크도 흐려지는지 비교한다.
const MULTI_ROW_DISABLED: [bool; 5] = [true, true, false, false, false];

/// 일괄 선택 예제의 옵션 목록.
const MULTI_ALL_OPTIONS: [&str; 10] = [
    "Waiting",
    "Ready",
    "Running",
    "Done",
    "Failed",
    "Skipped",
    "Blocked",
    "Retrying",
    "Cancelled",
    "Unknown",
];

/// 선택 여부가 다른 두 비활성 행은 일괄 선택·해제에서도 바뀌지 않아야 한다.
const MULTI_ALL_DISABLED: [bool; 8] = [true, true, false, false, false, false, false, false];

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
            // 한 번 선택하면 placeholder는 목록에서 사라진다.
            cluster(ui, theme, "Select (placeholder)", |ui| {
                let opts = ["Ctrl", "Alt", "Ctrl+Alt"];
                select_or_placeholder(
                    ui,
                    theme,
                    "gallery_select_placeholder",
                    &mut st.sel_placeholder,
                    &opts,
                    "Select a modifier",
                    field_md,
                    true,
                );
            });
            // 키보드로도 열기·행 이동·선택·닫기를 확인할 수 있다. 자동 스크린샷은 키를 주입하지 않는다.
            cluster(ui, theme, "Multi-select", |ui| {
                let opts = ["Waiting", "Ready", "Running", "Done", "Failed"];
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
                    None,
                    &labels,
                    None,
                    field_md,
                    true,
                );
            });
            // 선택 요약과 메뉴 행 양쪽에 긴 라벨을 넣어 말줄임을 비교한다.
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
                    None,
                    &labels,
                    None,
                    field_md,
                    true,
                );
            });
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
                    None,
                    &labels,
                    None,
                    field_md,
                    false,
                );
            });
            // 트리거는 활성인 채 지정된 행만 비활성화한다.
            cluster(ui, theme, "Multi-select (rows disabled)", |ui| {
                let opts = ["Waiting", "Ready", "Running", "Done", "Failed"];
                let labels = MultiSelectLabels {
                    none: "No status",
                    some: "{} selected",
                    all: "All statuses",
                };
                multi_select(
                    ui,
                    theme,
                    "gallery_multi_rows_disabled",
                    &mut st.multi_rows,
                    &opts,
                    Some(&MULTI_ROW_DISABLED),
                    &labels,
                    None,
                    field_md,
                    true,
                );
            });
            cluster(ui, theme, "Multi-select (all toggle)", |ui| {
                let opts = MULTI_ALL_OPTIONS;
                let labels = MultiSelectLabels {
                    none: "No status",
                    some: "{} selected",
                    all: "All statuses",
                };
                let all_toggle = MultiSelectAllToggle {
                    select_all: "Select all",
                    clear_all: "Clear all",
                };
                multi_select(
                    ui,
                    theme,
                    "gallery_multi_all",
                    &mut st.multi_all,
                    &opts,
                    None,
                    &labels,
                    Some(all_toggle),
                    field_md,
                    true,
                );
            });
            // 일괄 선택은 비활성 행을 제외하고 판단한다.
            cluster(
                ui,
                theme,
                "Multi-select (all toggle + rows disabled)",
                |ui| {
                    let opts = &MULTI_ALL_OPTIONS[..MULTI_ALL_DISABLED.len()];
                    let labels = MultiSelectLabels {
                        none: "No status",
                        some: "{} selected",
                        all: "All statuses",
                    };
                    let all_toggle = MultiSelectAllToggle {
                        select_all: "Select all",
                        clear_all: "Clear all",
                    };
                    multi_select(
                        ui,
                        theme,
                        "gallery_multi_all_masked",
                        &mut st.multi_all_masked,
                        opts,
                        Some(&MULTI_ALL_DISABLED),
                        &labels,
                        Some(all_toggle),
                        field_md,
                        true,
                    );
                },
            );
            // 높이 상한을 넘는 목록에서 내부 스크롤을 확인한다.
            cluster(ui, theme, "Multi-select (20 options)", |ui| {
                let opts = MULTI_SCROLL_OPTIONS;
                let labels = MultiSelectLabels {
                    none: "No column",
                    some: "{} columns",
                    all: "All columns",
                };
                multi_select(
                    ui,
                    theme,
                    "gallery_multi_scroll",
                    &mut st.multi_scroll,
                    &opts,
                    None,
                    &labels,
                    None,
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
            ("multi-select", "select trigger + checkbox rows"),
            ("row disabled", "state-disabled-opacity, no toggle"),
            ("all toggle", "opt-in accent row + separator, top of menu"),
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
            TokenChip::new(
                "separator",
                "all-toggle divider",
                egui::Color32::from(theme.separator),
            ),
        ],
    );
}
