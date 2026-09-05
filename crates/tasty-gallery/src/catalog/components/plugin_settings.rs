//! `plugin-settings` specimen — plugin-기여 설정 페이지의 **행 합성** 미러.
//!
//! 디자인 `(3) ui_kits/terminal/overlays/settings_window.jsx:240-248` 의 HTML viewer
//! 페이지: `Default zoom` / `Color scheme` / `Allow remote content` / `Sandbox scripts`
//! + Note. 각 행은 label 좌 / 컨트롤 우 (Row).
//!
//! 갤러리는 main 바이너리에 의존하지 않으므로, 본체 `src/view/settings/ui/tabs/appearance.rs`
//! 의 `plugin_setting_row` + `draw_plugin_{toggle,select,number}` 레이아웃·토큰을 공유 위젯
//! (`tasty_ui_widgets::{switch,select}`)로 **미러**한다 (갤러리 확립 패턴 — `prim_forms` /
//! `settings` specimen 과 동일).
//!
//! number 행: 디자인·본체·specimen 모두 text `Input`(mono, width xs) + suffix 로 일치한다
//! (본체 `draw_plugin_number` 가 `tasty_ui_widgets::Input` 를 쓰며, 과거의 `DragValue` 차이는
//! 해소됨).

use std::cell::RefCell;

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{Input, select, switch};

use crate::catalog::spec::{self, StageVariant, TokenChip};
use crate::catalog::widgets::dialog as kit;

/// 디자인 settings detail 영역(HTML viewer 페이지) 프레임 폭 근사.
const WIDTH: LogicalPx = LogicalPx(440.0);

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
        kit::frame_card(ui, theme, WIDTH, kit::panel_fill(theme), |ui| {
            kit::region_sym(ui, theme.spacing_md, theme.spacing_sm, |ui| {
                // 페이지 헤더 — Mono "HTML viewer".
                ui.label(
                    egui::RichText::new("HTML viewer")
                        .monospace()
                        .size(theme.font_size_micro.value())
                        .color(theme.text_muted().to_egui()),
                );
                STATE.with(|s| {
                    let st = &mut *s.borrow_mut();
                    // Default zoom — Input(mono, width xs) + "%" suffix (본체
                    // draw_plugin_number 미러: right_to_left 에서 suffix 가 가장 우측,
                    // 그 왼쪽에 입력 필드. 유효 f64 → 25..=500 clamp, 빈/무효는 무시,
                    // 비포커스 시 버퍼를 값으로 정규화).
                    row(ui, theme, "Default zoom:", |ui| {
                        ui.label(egui::RichText::new("%").color(theme.text_muted().to_egui()));
                        let resp = Input::new()
                            .mono(true)
                            .width(theme.field_width_xs.value())
                            .show(ui, theme, &mut st.zoom_buf);
                        if !resp.has_focus() {
                            let synced = if st.zoom.fract() == 0.0 {
                                format!("{:.0}", st.zoom)
                            } else {
                                format!("{}", st.zoom)
                            };
                            if st.zoom_buf != synced {
                                st.zoom_buf = synced;
                            }
                        } else if resp.changed()
                            && let Ok(parsed) = st.zoom_buf.trim().parse::<f64>()
                        {
                            st.zoom = parsed.clamp(25.0, 500.0);
                        }
                    });
                    // Color scheme — Select(width field_width_md).
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
                    // Allow remote content — Switch (off).
                    row(ui, theme, "Allow remote content:", |ui| {
                        switch(ui, theme, &mut st.allow_remote, None, true);
                    });
                    // Sandbox scripts — Switch (on).
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
                // Note.
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
