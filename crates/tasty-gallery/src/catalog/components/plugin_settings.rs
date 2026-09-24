//! 플러그인 설정 페이지를 공용 위젯으로 재현한 예제.
//! 숫자는 편집 중 그대로 두고 확정할 때만 범위를 제한한다.

use std::cell::RefCell;

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{Input, select, switch};

use crate::catalog::spec::{self, StageVariant, TokenChip};
use crate::catalog::widgets::dialog as kit;

/// 디자인 settings detail 영역(HTML viewer 페이지) 프레임 폭 근사.
const WIDTH: LogicalPx = LogicalPx(440.0);

/// Default zoom을 확정할 때 적용할 범위(본체 plugin 매니페스트의 min/max 재현).
const ZOOM_MIN: f64 = 25.0;
const ZOOM_MAX: f64 = 500.0;

/// Color scheme 선택지 (디자인 `Follow theme` / `Light` / `Dark`).
const SCHEME: &[&str] = &["Follow theme", "Light", "Dark"];

/// 회귀 방지 전용 — 디자인 미러 대상 아님. `field_width_md`(160px) 가용 폭을 넘는
/// 긴 옵션 라벨(Codex 플러그인 `default_approval_policy` 재현 케이스).
const LONG_TEXT_OPTS: &[&str] = &[
    "상속 (codex 기본값)",
    "Untrusted (신뢰되지 않은 명령만 승인 요청)",
    "On request (모델이 판단)",
    "Never (승인 프롬프트 없음)",
];

struct State {
    zoom: f64,
    /// number Input 의 프레임 간 편집 버퍼(본체 egui-memory 버퍼 미러). 초기 "100".
    zoom_buf: String,
    scheme_idx: usize,
    allow_remote: bool,
    sandbox: bool,
    long_text_idx: usize,
}

thread_local! {
    /// specimen 상호작용 상태(디자인 기본값: zoom 100 · Follow theme · remote off · sandbox on).
    static STATE: RefCell<State> = RefCell::new(State {
        zoom: 100.0,
        zoom_buf: String::from("100"),
        scheme_idx: 0,
        allow_remote: false,
        sandbox: true,
        long_text_idx: 3,
    });
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        // 설정 창 내부의 페이지이므로 팝업 그림자를 그리지 않는다.
        kit::frame_card_flat(ui, theme, WIDTH, kit::panel_fill(theme), |ui| {
            kit::region_sym(ui, theme.spacing_md, theme.spacing_sm, |ui| {
                ui.label(
                    egui::RichText::new("HTML viewer")
                        .monospace()
                        .size(theme.font_size_micro.value())
                        .color(theme.text_muted().to_egui()),
                );
                STATE.with(|s| {
                    let st = &mut *s.borrow_mut();
                    // 확정할 때만 25..=500 범위로 제한한다. suffix는 입력 오른쪽에 둔다.
                    row(ui, theme, "Default zoom:", |ui| {
                        ui.label(
                            egui::RichText::new("%")
                                .size(theme.font_size_term_sm.value())
                                .color(theme.text_muted().to_egui()),
                        );
                        let pending = zoom_out_of_range(&st.zoom_buf);
                        let resp = Input::new()
                            .mono(true)
                            .align(egui::Align::RIGHT)
                            .width(theme.field_width_xs.value())
                            .invalid(pending.is_some())
                            .show(ui, theme, &mut st.zoom_buf);
                        if resp.lost_focus() {
                            if let Ok(parsed) = st.zoom_buf.trim().parse::<f64>()
                                && parsed.is_finite()
                            {
                                st.zoom = parsed.clamp(ZOOM_MIN, ZOOM_MAX);
                            }
                            st.zoom_buf = format!("{:.0}", st.zoom);
                        } else if !resp.has_focus() {
                            let synced = format!("{:.0}", st.zoom);
                            if st.zoom_buf != synced {
                                st.zoom_buf = synced;
                            }
                        }
                    });
                    row(ui, theme, "Color scheme:", |ui| {
                        select(
                            ui,
                            theme,
                            "plugin_settings_scheme",
                            &mut st.scheme_idx,
                            SCHEME,
                            theme.field_width_md.value(),
                            true,
                        );
                    });
                    row(ui, theme, "Allow remote content:", |ui| {
                        switch(ui, theme, &mut st.allow_remote, None, true);
                    });
                    row(ui, theme, "Sandbox scripts:", |ui| {
                        switch(ui, theme, &mut st.sandbox, None, true);
                    });
                    // Approval policy (long text) — 디자인 미러 아님, select 긴 텍스트
                    // 말줄임 회귀 방지 전용 케이스.
                    row(ui, theme, "Approval policy:", |ui| {
                        select(
                            ui,
                            theme,
                            "plugin_settings_long_text",
                            &mut st.long_text_idx,
                            LONG_TEXT_OPTS,
                            theme.field_width_md.value(),
                            true,
                        );
                    });
                });
                ui.add_space(theme.spacing_sm.value());
                ui.label(
                    egui::RichText::new(
                        "Controls how the built-in HTML viewer renders previews opened from \
                             the terminal. Remote content is blocked by default.",
                    )
                    .size(theme.font_size_caption.value())
                    .color(theme.text_muted().to_egui()),
                );
            });
        });
    });

    spec::meta(
        ui,
        theme,
        &[
            ("row", "label 좌 / control 우 (right_to_left)"),
            ("row gap", "spacing_sm"),
            ("select width", "field_width_md"),
            ("switch", "28×16 track"),
            ("number", "Input(mono) + suffix"),
        ],
        &[
            TokenChip::new("text", "row label", theme.text_primary().to_egui()),
            TokenChip::new("text-muted", "suffix · note", theme.text_muted().to_egui()),
            TokenChip::new(
                "accent-primary",
                "switch on / select",
                theme.accent_primary().to_egui(),
            ),
            TokenChip::new(
                "border-strong",
                "frame edge",
                theme.border_strong().to_egui(),
            ),
        ],
    );
}

/// 본체 `plugin_setting_row` 미러 — label 좌(`th.text`) / control 우(`right_to_left`),
/// 앞에 `spacing_sm` 여백.
fn row(ui: &mut egui::Ui, theme: &Theme, label: &str, control: impl FnOnce(&mut egui::Ui)) {
    ui.add_space(theme.spacing_sm.value());
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).color(theme.text_primary().to_egui()));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), control);
    });
}

/// 확정 시 범위 제한으로 값이 바뀔 입력인지 확인한다. 해당 입력은 오류 테두리로 표시한다.
fn zoom_out_of_range(buf: &str) -> Option<f64> {
    let typed = buf.trim().parse::<f64>().ok()?;
    if !typed.is_finite() {
        return None;
    }
    let settled = typed.clamp(ZOOM_MIN, ZOOM_MAX);
    (settled != typed).then_some(settled)
}
