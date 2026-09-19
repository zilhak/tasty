//! `settings-number` specimen — 설정 창의 **숫자 한 모양** 미러.
//!
//! 디자인 `gallery/overlays-windows.jsx` 의 "Numbers in settings — one shape".
//! 설정 안에 숫자 컨트롤이 셋(정적 suffix 를 단 mono Input · drag 숫자 · 제안된
//! stepper) 있었고 **첫째로 통일**했다. 상태 셋을 그대로 전시한다 — default ·
//! out of range · disabled.
//!
//! 갤러리는 main 바이너리에 의존하지 않으므로 본체
//! `src/view/settings/ui/tabs/number.rs` 의 `number_field` 를 공유 위젯
//! (`tasty_ui_widgets::Input`)으로 **미러**한다(갤러리 확립 패턴). 확정 판정 자체
//! (`commit`)는 본체 쪽 단위 테스트가 든다 — 여기서는 **보이는 것**만 고정한다.

use std::cell::RefCell;

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::Input;

use crate::catalog::spec::{self, StageVariant, TokenChip};

/// 디자인 specimen 의 행 폭(`width: 420`).
const ROW_WIDTH: LogicalPx = LogicalPx(420.0);

/// 이 specimen 이 전시하는 줌 범위 — 디자인 문구가 그대로 쓰는 25~200 %.
const ZOOM_MIN: f64 = 25.0;
const ZOOM_MAX: f64 = 200.0;

struct State {
    /// 세 행의 편집 버퍼. 본체가 egui 메모리에 두는 것과 같은 자리다.
    bufs: [String; 3],
}

thread_local! {
    /// 디자인 상태 셋의 초기 글자 — 범위 안 · 범위 밖 · disabled.
    static STATE: RefCell<State> = RefCell::new(State {
        bufs: [
            String::from("150"),
            String::from("420"),
            String::from("100"),
        ],
    });
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = theme.spacing_md.value();
            STATE.with(|s| {
                let st = &mut *s.borrow_mut();
                for (i, label) in ["default", "out of range — clamp on commit", "disabled"]
                    .iter()
                    .enumerate()
                {
                    let enabled = i != 2;
                    row(ui, theme, label, &mut st.bufs[i], enabled);
                }
            });
        });
    });

    spec::meta(
        ui,
        theme,
        &[
            ("control", "Input — 새 컴포넌트 아님"),
            ("font", "mono · 자릿수 우측 정렬"),
            ("width", "field_width_xs (90)"),
            ("suffix", "필드 밖 정적 텍스트, muted"),
            ("clamp", "확정(blur / ↵) 때만 — 치는 중엔 안 건드린다"),
            ("out of range", "danger 테두리 + 범위 한 줄"),
        ],
        &[
            TokenChip::new(
                "accent-danger",
                "범위 밖 테두리 · 줄",
                theme.accent_danger().to_egui(),
            ),
            TokenChip::new("text-muted", "단위 suffix", theme.text_muted().to_egui()),
            TokenChip::new(
                "text-disabled",
                "disabled 라벨 · 단위",
                theme.text_disabled().to_egui(),
            ),
        ],
    );
}

/// 한 행 — 상태 캡션 + (라벨 · 필드 · 단위) + (범위 밖이면) 한 줄.
fn row(ui: &mut egui::Ui, theme: &Theme, caption: &str, buf: &mut String, enabled: bool) {
    let settled = out_of_range(buf);
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = theme.spacing_xs.value();
        ui.label(
            egui::RichText::new(caption)
                .size(theme.font_size_caption.value())
                .color(theme.text_muted().to_egui()),
        );
        ui.allocate_ui(
            egui::vec2(ROW_WIDTH.value(), theme.input_height().value()),
            |ui| {
                // 오른쪽부터 채운다 — 단위 · 필드 · 라벨. 폭을 손으로 나누지 않아야
                // 라벨이 남는 자리를 그대로 갖는다.
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
                    let muted = if enabled {
                        theme.text_muted()
                    } else {
                        theme.text_disabled()
                    };
                    ui.label(
                        egui::RichText::new("%")
                            .size(theme.font_size_caption.value())
                            .color(muted),
                    );
                    Input::new()
                        .mono(true)
                        .align(egui::Align::RIGHT)
                        .width(theme.field_width_xs.value())
                        .enabled(enabled)
                        .invalid(settled.is_some())
                        .show(ui, theme, buf);
                    let ink = if enabled {
                        theme.text_secondary()
                    } else {
                        theme.text_disabled()
                    };
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.label(egui::RichText::new("Default zoom").color(ink));
                    });
                });
            },
        );
        if let Some(settled) = settled {
            ui.label(
                egui::RichText::new(format!(
                    "Between {ZOOM_MIN:.0} and {ZOOM_MAX:.0}. Commits as {settled:.0}."
                ))
                .size(theme.font_size_caption.value())
                .color(theme.accent_danger().to_egui()),
            );
        }
    });
}

/// 지금 친 글자가 확정되면 값이 끌려가는가. 본체 `number::out_of_range` 미러.
fn out_of_range(buf: &str) -> Option<f64> {
    let typed = buf.trim().parse::<f64>().ok()?;
    if !typed.is_finite() {
        return None;
    }
    let settled = typed.clamp(ZOOM_MIN, ZOOM_MAX);
    (settled != typed).then_some(settled)
}
