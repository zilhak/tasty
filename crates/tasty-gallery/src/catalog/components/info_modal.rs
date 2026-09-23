//! 부팅 안내·오류 메시지 팝업의 정적 예제.
//! 본문은 높이를 제한해 스크롤하고 확인 버튼은 오른쪽 아래에 둔다.
//! 본체와 달리 버튼은 공용 tasty-ui-widgets::Button을 사용한다.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{Button, ButtonVariant};

use crate::catalog::popup_frame::{self, ContentInset, TitleButtons};
use crate::catalog::spec::{self, StageVariant, TokenChip};

/// 본체 `info_modal.rs` 의 `DEFAULT_WIDTH`.
const WIDTH: LogicalPx = LogicalPx(440.0);
/// 본체 `info_modal.rs` 의 `MIN_HEIGHT`.
const MIN_HEIGHT: LogicalPx = LogicalPx(140.0);
/// 본체 `info_modal.rs` 의 `MAX_HEIGHT`.
const MAX_HEIGHT: LogicalPx = LogicalPx(360.0);

/// 본체 `info_modal_sizer` 와 같은 규칙으로 높이를 낸다 —
/// 60자/줄 가정 · 줄높이 = body 폰트 × 1.5 · 하단 버튼 영역 48 · clamp.
fn sizer_height(theme: &Theme, body: &str) -> LogicalPx {
    let approx_lines = (body.chars().count() as f32 / 60.0).ceil().max(2.0);
    let line_h = theme.font_size_body * 1.5;
    let footer_h = theme.item_height_interactive + theme.spacing_lg + theme.spacing_xs;
    (popup_frame::TITLE_BAR_HEIGHT
        + popup_frame::CONTENT_MARGIN * 2.0
        + line_h * approx_lines
        + footer_h)
        .clamp(MIN_HEIGHT, MAX_HEIGHT)
}

/// 모달 1장. `extra` 는 [OK] 왼쪽에 붙는 추가 버튼 라벨.
fn modal(ui: &mut egui::Ui, theme: &Theme, title: &str, body: &str, extra: Option<&str>) {
    let h = sizer_height(theme, body);
    popup_frame::draw(
        ui,
        theme,
        title,
        WIDTH,
        h,
        ContentInset::INSET,
        TitleButtons::CLOSE,
        // 창 중앙 팝업이므로 modal 그림자를 쓴다.
        Some(theme.shadow_modal()),
        |ui| {
            // 본체와 같은 규칙 — 본문이 넘치면 스크롤하고 버튼 행은 자리를 지킨다.
            let footer_h =
                (theme.item_height_interactive + theme.spacing_lg + theme.spacing_xs).value();
            egui::ScrollArea::vertical()
                .max_height((ui.available_height() - footer_h).max(0.0))
                .auto_shrink([false, true])
                .drag_to_scroll(false)
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(body)
                            .size(theme.font_size_body.value())
                            .color(theme.text_primary().to_egui()),
                    );
                });
            ui.with_layout(egui::Layout::bottom_up(egui::Align::RIGHT), |ui| {
                ui.add_space(theme.spacing_xs.value());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    Button::new("OK")
                        .variant(ButtonVariant::Primary)
                        .show(ui, theme);
                    if let Some(label) = extra {
                        Button::new(label)
                            .variant(ButtonVariant::Secondary)
                            .show(ui, theme);
                    }
                });
            });
        },
    );
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        spec::cluster(ui, theme, "Notice (OK only)", |ui| {
            modal(
                ui,
                theme,
                "Theme fallback",
                "The configured theme could not be read, so the built-in Mocha theme is in use. \
                 Your theme file is left untouched.",
                None,
            )
        });
        spec::cluster(ui, theme, "With follow-up action", |ui| {
            modal(
                ui,
                theme,
                "Some permissions are not granted",
                "Full Disk Access and screen recording are not granted. Settings › General › \
                 Permissions shows the current state of each; Full Disk Access has to be added \
                 in System Settings because no app can request it.",
                Some("Open System Settings"),
            )
        });
    });

    spec::meta(
        ui,
        theme,
        &[
            ("frame", "440px · height 140..360 (body 길이로 산출)"),
            ("title", "큐 head 의 title — 타이틀바에 실린다"),
            (
                "body",
                "font-size-body · text-primary · 좌우 8 / 상하 4 inset",
            ),
            ("footer", "bottom-up RIGHT · [OK] 가 가장 오른쪽"),
        ],
        &[
            TokenChip::new(
                "surface-raised",
                "popup frame",
                theme.surface_raised().to_egui(),
            ),
            TokenChip::new(
                "surface-hover",
                "title bar",
                theme.surface_hover().to_egui(),
            ),
            TokenChip::new("text-primary", "body", theme.text_primary().to_egui()),
        ],
    );

    spec::note(
        ui,
        theme,
        "큐 모델이다 — 여러 건이 쌓이면 [OK] 마다 다음 메시지로 넘어가고, 마지막을 확인해야 \
         popup 이 닫힌다. X 로 닫아도 head 를 pop 해 남은 안내가 유실되지 않는다. \
         추가 버튼(설정 패널 열기 등)은 모달을 닫지 않는다 — 안내를 다시 읽을 수 있어야 한다.",
    );
}
