//! Rename popup 데모 (Tier 3 재분류 항목).
//!
//! 본체 `src/adapters/ui/dialog.rs::draw_rename_popup_view` 가 표현하는 시각
//! 상태를 mock props 로 재현. 본체와 *시각 동일* 하지만 gallery 가 본체 binary
//! 에 의존할 수 없으므로 view 로직은 로컬 미러 (POC 패턴 —
//! `.claude-workspace/conductor/tier-3-props-extraction-pattern.md`).
//!
//! 본체 wrapper 3 종 (workspace name / workspace subtitle / tab name) 은 모두
//! 같은 view 를 호출하므로 RenameTarget 차이는 시각상 *제목* 뿐이다. 카탈로그는
//! 각 케이스 별로 buffer 만 다르게 mock 한다.
//!
//! 대표 상태:
//! - 빈 buffer (placeholder 상황)
//! - 짧은 이름 (전형적)
//! - 긴 영문 이름 (overflow)
//! - 한글 입력 (CJK 폰트)
//! - 매우 긴 이름 (잘림)
//!
//! Buffer 는 thread-local `RefCell<Vec<String>>` 으로 케이스별 따로 보관 — 매
//! 프레임 호출되는 catalog `draw` 가 stateless 라 buffer 가 사라지지 않게 한다.

use std::cell::RefCell;
use tasty_type_appearance::theme::Theme;

struct RenamePopupProps<'a> {
    theme: &'a Theme,
    buffer: &'a mut String,
    save_label: &'a str,
    cancel_label: &'a str,
    body_font_size: f32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RenamePopupAction {
    None,
    Cancel,
    Confirm(String),
}

/// 본체 `draw_rename_popup_view` 의 시각 미러 (gallery 측 복제).
fn draw_rename_popup_view(
    ui: &mut egui::Ui,
    props: &mut RenamePopupProps<'_>,
) -> RenamePopupAction {
    let ctx = ui.ctx().clone();
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        return RenamePopupAction::Cancel;
    }

    let resp = ui.add_sized(
        [ui.available_width(), 22.0],
        egui::TextEdit::singleline(props.buffer)
            .font(egui::FontId::proportional(props.body_font_size))
            .margin(egui::Margin::symmetric(4, 2)),
    );

    // Gallery 는 자동 focus 를 적용하지 않는다 — 다른 케이스의 input field 가
    // 동시에 focus 를 가져가 사용자 혼란을 일으킴. focus 는 직접 클릭으로만.

    let mut confirm = false;
    let mut cancel = false;

    if resp.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
        confirm = true;
    }

    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button(props.cancel_label).clicked() {
                cancel = true;
            }
            if ui.button(props.save_label).clicked() {
                confirm = true;
            }
        });
    });

    let _theme = props.theme; // theme 토큰은 TextEdit 내부 styling 에서 간접 사용.

    if confirm {
        return RenamePopupAction::Confirm(props.buffer.clone());
    }
    if cancel {
        return RenamePopupAction::Cancel;
    }
    RenamePopupAction::None
}

thread_local! {
    static BUFFERS: RefCell<Vec<String>> = RefCell::new(vec![
        String::new(),
        "default".to_string(),
        "very-long-workspace-name-that-overflows-the-field".to_string(),
        "한글 이름 예시".to_string(),
        "이것은 잘리는 매우 매우 매우 긴 텍스트입니다 — gallery 회귀 확인용".to_string(),
    ]);
}

fn case_title(ui: &mut egui::Ui, theme: &Theme, title: &str) {
    ui.label(
        egui::RichText::new(title)
            .strong()
            .color(egui::Color32::from(theme.text)),
    );
    ui.add_space(2.0);
}

fn case_box(ui: &mut egui::Ui, theme: &Theme, title: &str, idx: usize) {
    case_title(ui, theme, title);
    egui::Frame::group(ui.style())
        .corner_radius(theme.corner_radius.value())
        .show(ui, |ui| {
            ui.set_min_width(280.0);
            ui.set_max_width(280.0);
            BUFFERS.with(|cell| {
                let mut buffers = cell.borrow_mut();
                if let Some(buffer) = buffers.get_mut(idx) {
                    let mut props = RenamePopupProps {
                        theme,
                        buffer,
                        save_label: "Save",
                        cancel_label: "Cancel",
                        body_font_size: 12.0,
                    };
                    let _action = draw_rename_popup_view(ui, &mut props);
                }
            });
        });
    ui.add_space(16.0);
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    ui.label(
        egui::RichText::new("draw_rename_popup_view — workspace/tab rename input (5 mock cases)")
            .small()
            .color(egui::Color32::from(theme.subtext0)),
    );
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new(
            "Wrapper: src/adapters/ui/dialog.rs::draw_rename_popup (PopupDef draw_fn)",
        )
        .small()
        .color(egui::Color32::from(theme.subtext0)),
    );
    ui.add_space(12.0);

    case_box(ui, theme, "Case 1 — Empty buffer (placeholder)", 0);
    case_box(ui, theme, "Case 2 — Short name (typical)", 1);
    case_box(ui, theme, "Case 3 — Long English name (overflow)", 2);
    case_box(ui, theme, "Case 4 — Korean input (CJK)", 3);
    case_box(ui, theme, "Case 5 — Very long Korean text (truncation)", 4);

    ui.label(
        egui::RichText::new(
            "Note: 본체 view 는 첫 프레임에 자동 focus + 전체 선택을 적용하지만, \
             gallery 는 다섯 케이스가 동시에 노출돼 focus 경합이 일어나므로 \
             auto-focus 를 의도적으로 끔. 버퍼는 thread-local 로 케이스별 보관.",
        )
        .small()
        .color(egui::Color32::from(theme.subtext0)),
    );
}
