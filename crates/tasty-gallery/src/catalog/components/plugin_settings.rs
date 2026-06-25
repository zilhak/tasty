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
//! ⚠️ number 행 차이: 디자인은 text `Input`(mono) 이지만 본체가 egui `DragValue` 를 쓰므로
//!    specimen 도 `DragValue` 로 미러한다 (본체 `draw_plugin_number` 와의 일치를 우선).

use std::cell::RefCell;

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{select, switch};

use crate::catalog::spec::{self, StageVariant, TokenChip};
use crate::catalog::widgets::dialog as kit;

/// 디자인 settings detail 영역(HTML viewer 페이지) 프레임 폭 근사.
const WIDTH: f32 = 440.0;

/// Color scheme 선택지 (디자인 `Follow theme` / `Light` / `Dark`).
const SCHEME: &[&str] = &["Follow theme", "Light", "Dark"];

struct State {
    zoom: f64,
    scheme_idx: usize,
    allow_remote: bool,
    sandbox: bool,
}

thread_local! {
    /// specimen 상호작용 상태(디자인 기본값: zoom 100 · Follow theme · remote off · sandbox on).
    static STATE: RefCell<State> = const {
        RefCell::new(State {
            zoom: 100.0,
            scheme_idx: 0,
            allow_remote: false,
            sandbox: true,
        })
    };
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        kit::frame_card(ui, theme, WIDTH, kit::panel_fill(theme), |ui| {
            kit::region_sym(
                ui,
                theme.spacing_md.value(),
                theme.spacing_sm.value(),
                |ui| {
                    // 페이지 헤더 — Mono "HTML viewer".
                    ui.label(
                        egui::RichText::new("HTML viewer")
                            .monospace()
                            .size(theme.font_size_micro.value())
                            .color(theme.text_muted().to_egui()),
                    );
                    STATE.with(|s| {
                        let st = &mut *s.borrow_mut();
                        // Default zoom — DragValue + "%" suffix (본체 draw_plugin_number 미러:
                        // right_to_left 에서 suffix 가 가장 우측, 그 왼쪽에 입력 필드).
                        row(ui, theme, "Default zoom:", |ui| {
                            ui.label(egui::RichText::new("%").color(theme.text_muted().to_egui()));
                            ui.add(
                                egui::DragValue::new(&mut st.zoom)
                                    .range(25.0..=500.0)
                                    .custom_formatter(|n, _| {
                                        if n.fract() == 0.0 {
                                            format!("{n:.0}")
                                        } else {
                                            format!("{n}")
                                        }
                                    }),
                            );
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
                },
            );
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
            ("number", "DragValue + suffix"),
        ],
        &[
            TokenChip::new("text", "row label", theme.text.to_egui()),
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
        ui.label(egui::RichText::new(label).color(theme.text.to_egui()));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), control);
    });
}
