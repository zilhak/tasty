//! Two-depth layout — 좌측 sub-menu 패널 + 우측 콘텐츠 idiom.
//!
//! 본체 settings 의 Appearance / Keybindings 탭, 갤러리 `layout_2depth` 가 공유하는 패턴.
//! - 좌측: 고정 폭 (`tokens::SUB_TAB_PANEL_WIDTH`) `Frame` + `crust` 배경 + `surface0` 1px 보더.
//! - 우측: `set_max_height(available_height)` 만 걸린 vertical 영역.
//! - 좌·우 사이 `tokens::PANEL_SPACING` (8px) 의 horizontal gap.
//!
//! `available_height` 는 호출자가 계산해서 넘긴다 — 모달/패널마다 header/footer 보정값이 달라
//! widget 측이 추정할 수 없다.

use tasty_type_appearance::theme::Theme;

use crate::tokens;

/// 좌측 sub-menu 패널 + 우측 콘텐츠 영역을 그린다.
///
/// `left` 클로저 안에서는 `ui.selectable_label(...)` 같은 sub-tab 라벨 리스트를 그린다.
/// `content` 클로저는 우측 vertical 영역에서 호출된다 — `set_max_height` 가 이미 걸려
/// 있으므로 추가로 높이 제약을 걸 필요 없다.
pub fn two_depth_layout(
    ui: &mut egui::Ui,
    theme: &Theme,
    available_height: f32,
    left: impl FnOnce(&mut egui::Ui),
    content: impl FnOnce(&mut egui::Ui),
) {
    two_depth_layout_inner(ui, theme, available_height, None, left, content);
}

/// `two_depth_layout` + 좌측 패널 상단에 L2 섹션 필터 입력 슬롯.
///
/// `filter` 는 검색 문자열의 mutable backing store. 입력 박스는 widget 이 그리지만
/// *실제 항목 필터링은 호출자의 `left` 클로저* 가 `*filter` 를 읽어 수행한다 —
/// widget 은 어떤 항목이 있는지 모르기 때문. `placeholder` 는 hint 텍스트.
pub fn two_depth_layout_filtered(
    ui: &mut egui::Ui,
    theme: &Theme,
    available_height: f32,
    filter: &mut String,
    placeholder: &str,
    left: impl FnOnce(&mut egui::Ui),
    content: impl FnOnce(&mut egui::Ui),
) {
    two_depth_layout_inner(
        ui,
        theme,
        available_height,
        Some((filter, placeholder)),
        left,
        content,
    );
}

fn two_depth_layout_inner(
    ui: &mut egui::Ui,
    theme: &Theme,
    available_height: f32,
    filter: Option<(&mut String, &str)>,
    left: impl FnOnce(&mut egui::Ui),
    content: impl FnOnce(&mut egui::Ui),
) {
    ui.horizontal_top(|ui| {
        egui::Frame::new()
            .fill(egui::Color32::from(theme.crust))
            .stroke(egui::Stroke::new(
                tokens::PANEL_STROKE_WIDTH,
                egui::Color32::from(theme.surface0),
            ))
            .corner_radius(tokens::PANEL_CORNER_RADIUS)
            .inner_margin(egui::Margin::symmetric(
                tokens::PANEL_INNER_MARGIN,
                tokens::PANEL_INNER_MARGIN,
            ))
            .show(ui, |ui| {
                ui.set_width(tokens::SUB_TAB_PANEL_WIDTH);
                ui.set_min_height(available_height);

                ui.vertical(|ui| {
                    if let Some((filter, placeholder)) = filter {
                        ui.add(
                            egui::TextEdit::singleline(filter)
                                .hint_text(placeholder)
                                .desired_width(f32::INFINITY),
                        );
                        ui.add_space(tokens::PANEL_SPACING);
                        ui.separator();
                        ui.add_space(tokens::PANEL_SPACING);
                    }
                    left(ui);
                });
            });

        ui.add_space(tokens::PANEL_SPACING);

        ui.vertical(|ui| {
            ui.set_max_height(available_height);
            content(ui);
        });
    });
}
