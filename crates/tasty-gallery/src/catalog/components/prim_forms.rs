//! Select, Checkbox, Switch의 상태별 예제. MultiSelect 는 `prim_multiselect`,
//! SearchableSelect 의 상태 전체는 `prim_searchable_select` 에 있고 여기에는 같은 줄 비교용 live 칸만 둔다.

use std::cell::RefCell;

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{
    SearchableSelect, SearchableSelectLabels, SearchableSelectOption, checkbox, select,
    select_or_placeholder, switch,
};

use crate::catalog::spec::{StageVariant, TokenChip, cluster, meta, stage};

const FONTS: [SearchableSelectOption<'static>; 4] = [
    SearchableSelectOption::new("D2Coding"),
    SearchableSelectOption::new("JetBrains Mono"),
    SearchableSelectOption::new("Fira Code"),
    SearchableSelectOption::new("Cascadia Code"),
];
const PIN: [SearchableSelectOption<'static>; 1] = [SearchableSelectOption::with_value(
    "",
    "Default (monospace)",
)];
const FONT_LABELS: SearchableSelectLabels<'static> = SearchableSelectLabels {
    placeholder: "Choose a font",
    search_placeholder: "Search fonts",
    empty: "No matching fonts",
    loading: "Loading fonts…",
};

thread_local! {
    static STATE: RefCell<FormState> = const {
        RefCell::new(FormState {
            sel: 0,
            sel_long: 3,
            sel_placeholder: None,
            font: None,
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
    /// SearchableSelect 의 값. 처음엔 비어 placeholder 가 보인다.
    font: Option<String>,
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
            cluster(ui, theme, "Searchable Select", |ui| {
                SearchableSelect::new("gallery_searchable_select", &FONTS, FONT_LABELS)
                    .pinned(&PIN)
                    .width(theme.field_width_md)
                    .show(ui, theme, &mut st.font);
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
            (
                "searchable select",
                "same 28 trigger and chevron as Select; open list has a search field (see SearchableSelect)",
            ),
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
            TokenChip::without_color("border-default", "outline"),
        ],
    );
}
