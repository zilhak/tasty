//! 설정의 override 한 행 — 라벨 열(설정 행 격자의 열 폭) · 컨트롤(자기 필드 폭) · 뒤따르는 "Use default".
//! 색 override 행과 같은 형태로, 글꼴 override 격자가 이 행을 쌓는다.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;

use crate::toggle::checkbox_width;

/// override 한 행의 칸. 한 클로저가 세 칸을 차례로 그려 같은 override 필드를 빌린다.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OverrideCell {
    Label,
    Control,
    UseDefault,
}

/// 체크박스가 컨트롤 오른쪽에 들어가는지. 들어가지 않으면 컨트롤 아래 줄로 내린다.
pub fn override_row_fits(
    theme: &Theme,
    label_col: LogicalPx,
    available: LogicalPx,
    control_w: LogicalPx,
    check_w: LogicalPx,
) -> bool {
    let gap = theme.settings_label_gap();
    label_col + gap + control_w + gap + check_w <= available
}

/// override 한 행을 그린다. 라벨 칸은 `label_col` 폭(보통 [`crate::settings_label_column`] 의 값)이고,
/// 라벨은 그 칸 안에 [`crate::settings_label_cell`] 로 그린다. 칸 사이 `settings-label-gap`, 위아래 `space-xs`, 최소 높이
/// `settings-row-min-height`. 체크박스가 남은 폭에 들어가지 않으면 컨트롤 아래 줄로 내려가며
/// 줄 사이는 `space-xs`다. `check_label` 은 체크박스 폭을 재는 데만 쓴다.
pub fn override_row(
    ui: &mut egui::Ui,
    theme: &Theme,
    label_col: LogicalPx,
    control_w: LogicalPx,
    check_label: &str,
    mut cell: impl FnMut(&mut egui::Ui, OverrideCell),
) {
    let label_w = label_col;
    let gap = theme.settings_label_gap();
    let row_h = theme.settings_row_min_height().value();
    let check_w = LogicalPx(checkbox_width(ui, theme, check_label));
    let fits = override_row_fits(
        theme,
        label_w,
        LogicalPx(ui.available_width()),
        control_w,
        check_w,
    );
    let pad_y = theme.spacing_xs.value().round() as i8;
    egui::Frame::new()
        .inner_margin(egui::Margin {
            top: pad_y,
            bottom: pad_y,
            ..egui::Margin::ZERO
        })
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = theme.spacing_xs.value();
            ui.horizontal(|ui| {
                ui.set_min_height(row_h);
                ui.spacing_mut().item_spacing.x = gap.value();
                ui.allocate_ui_with_layout(
                    egui::vec2(label_w.value(), row_h),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.set_width(label_w.value());
                        cell(ui, OverrideCell::Label);
                    },
                );
                ui.allocate_ui_with_layout(
                    egui::vec2(control_w.value(), row_h),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| cell(ui, OverrideCell::Control),
                );
                if fits {
                    cell(ui, OverrideCell::UseDefault);
                }
            });
            if !fits {
                ui.horizontal(|ui| {
                    ui.add_space((label_w + gap).value());
                    cell(ui, OverrideCell::UseDefault);
                });
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_checkbox_drops_under_the_control_only_when_it_does_not_fit() {
        let th = tasty_themes::mocha_fallback();
        let control = th.field_width_lg;
        let check = LogicalPx(100.0);
        let gap = th.settings_label_gap();
        for col in [th.settings_label_width(), th.settings_label_max_width()] {
            let exact = col + gap + control + gap + check;
            assert!(override_row_fits(&th, col, exact, control, check));
            assert!(!override_row_fits(
                &th,
                col,
                exact - LogicalPx(1.0),
                control,
                check
            ));
        }
    }
}
