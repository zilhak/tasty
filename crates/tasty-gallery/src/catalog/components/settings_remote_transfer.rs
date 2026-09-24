//! 원격 파일 수신 폴더와 용량 상한 설정의 예제. 실제 설정 저장은 하지 않는다.

use std::cell::RefCell;
use tasty_type_geometry::length::LogicalPx;

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{Button, ButtonVariant, ControlSize, Input};

use crate::catalog::icons;
use crate::catalog::spec::{self, StageVariant, TokenChip};
use crate::catalog::widgets::dialog as kit;

/// 디자인 settings 콘텐츠 컬럼 근사 프레임 폭(settings_handler 와 동일).
const WIDTH: LogicalPx = LogicalPx(560.0);
/// jsx `gridTemplateColumns: "150px 1fr"` 라벨 컬럼 폭.
const LABEL_COL_W: LogicalPx = LogicalPx(150.0);
/// 본체와 같은 field_width_xs를 사용한다.
fn size_input_width(theme: &Theme) -> f32 {
    theme.field_width_xs.value()
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State {
        dir: String::new(),
        max: String::from("500"),
    });
}

struct State {
    dir: String,
    max: String,
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        // 설정 창 내부 콘텐츠이므로 그림자를 추가하지 않는다.
        kit::frame_card_flat(ui, theme, WIDTH, kit::panel_fill(theme), |ui| {
            kit::region_sym(ui, theme.spacing_lg, theme.spacing_md, |ui| {
                ui.spacing_mut().item_spacing.y = theme.spacing_sm.value();
                mono_head(ui, theme, "Received files");

                STATE.with(|s| {
                    let st = &mut *s.borrow_mut();

                    xfer_row(ui, theme, "Save folder", |ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
                            // specimen — 클릭 응답 불필요, 그리기만(폴더 피커는 host 소유).
                            let _ = Button::new("Browse…")
                                .variant(ButtonVariant::Secondary)
                                .size(ControlSize::Sm)
                                .leading_icon(&|ui, rect, c| {
                                    icons::FOLDER.image(rect.width(), c).paint_at(ui, rect);
                                })
                                .show(ui, theme);
                            Input::new()
                                .mono(true)
                                .placeholder("~/.tasty/transfers/")
                                .show(ui, theme, &mut st.dir);
                        });
                    });
                    row_desc(
                        ui,
                        theme,
                        "Where files received from a remote workspace are saved.",
                    );
                    separator_line(ui, theme);

                    xfer_row(ui, theme, "Maximum size", |ui| {
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
                            Input::new().mono(true).width(size_input_width(theme)).show(
                                ui,
                                theme,
                                &mut st.max,
                            );
                            ui.label(
                                egui::RichText::new("MiB")
                                    .monospace()
                                    .size(theme.font_size_caption.value())
                                    .color(theme.text_muted().to_egui()),
                            );
                        });
                    });
                    row_desc(
                        ui,
                        theme,
                        "Total the folder may hold. A transfer that would push it past this \
                             limit is rejected before it starts.",
                    );
                });
            });
        });
    });

    spec::meta(
        ui,
        theme,
        &[
            ("L2 position", "5th — after Overlay"),
            ("rows", "Save folder · Maximum size"),
            ("row grid", "150px label · control · gap 12"),
            ("row height", "settings-row-min-height"),
            ("folder row", "mono Input + Browse… (secondary · folder)"),
            (
                "size row",
                "numeric Input 90 (field-width-xs) + static mono “MiB”",
            ),
        ],
        &[
            TokenChip::new(
                "settings-row-min-height",
                "row height",
                theme.surface_active().to_egui(),
            ),
            TokenChip::new("separator", "row divider", theme.separator.to_egui()),
            TokenChip::new(
                "text-muted",
                "descriptions + unit",
                theme.text_muted().to_egui(),
            ),
        ],
    );

    spec::note(
        ui,
        theme,
        "These rows show the receiving folder and its total capacity limit. MiB is a static unit label outside the numeric input. The host rejects a new transfer when the current folder usage plus the incoming size exceeds the limit.",
    );
}

/// jsx `Mono` — mono 10 uppercase, text-muted (settings_handler `mono_head` 관례).
fn mono_head(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.label(
        egui::RichText::new(text.to_uppercase())
            .monospace()
            .size(theme.font_size_micro.value())
            .color(theme.text_muted().to_egui()),
    );
}

/// settings-row 한 행: 150px 좌측 라벨(수직 중앙) + `spacing_md`(12) gap + 컨트롤.
/// 행 높이는 `settings_row_min_height`(32) 하한 (host `settings_row` 와 동형).
fn xfer_row(ui: &mut egui::Ui, theme: &Theme, label: &str, control: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.set_min_height(theme.settings_row_min_height().value());
        ui.spacing_mut().item_spacing.x = 0.0;
        let (lr, _) = ui.allocate_exact_size(
            egui::vec2(LABEL_COL_W.value(), theme.settings_row_min_height().value()),
            egui::Sense::hover(),
        );
        ui.painter().text(
            egui::pos2(lr.left(), lr.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(theme.font_size_body.value()),
            theme.text_primary().to_egui(),
        );
        ui.add_space(theme.spacing_md.value());
        control(ui);
    });
}

/// 행 아래 muted 설명줄 (caption · text-muted).
fn row_desc(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .size(theme.font_size_caption.value())
            .color(theme.text_muted().to_egui()),
    );
}

/// 행 사이 1px separator (jsx `borderTop: 1px solid separator`).
fn separator_line(ui: &mut egui::Ui, theme: &Theme) {
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(w, theme.border_width.value()),
        egui::Sense::hover(),
    );
    ui.painter().hline(
        rect.x_range(),
        rect.center().y,
        egui::Stroke::new(theme.border_width.value(), theme.separator.to_egui()),
    );
}
