//! 가로 탭 목록과 넘친 영역을 이동하는 화살표. id_salt는 호출자별로 달라야 한다.
//! SVG 로더가 미리 설치돼 있어야 한다. 화살표는 탭 위에 겹쳐 그린다.
//! 현재 갤러리 레이아웃 예제에서 사용하며 본체 설정 창은 제목을 포함한 별도 밴드를 그린다.

use egui::Color32;

/// 한 step 스크롤 거리 (평균 탭 너비 ~80px 기준).
const SCROLL_STEP: f32 = 80.0;

/// 탭을 선택하면 active를 갱신한다. 목록이 넘치면 화살표로 SCROLL_STEP만큼 이동한다.
pub fn horizontal_tab_bar_with_arrows<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    id_salt: &str,
    tabs: &[(T, &str)],
    active: &mut T,
) {
    let output = egui::ScrollArea::horizontal()
        .id_salt(id_salt)
        .auto_shrink([false, true])
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
        .max_width(ui.available_width())
        // 탭 클릭 중 드래그가 스크롤로 바뀌지 않도록 패닝을 끈다.
        .drag_to_scroll(false)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                for (tab, label) in tabs {
                    let selected = *active == *tab;
                    if ui.selectable_label(selected, *label).clicked() {
                        *active = *tab;
                    }
                }
            });
        });

    let viewport_w = output.inner_rect.width();
    let content_w = output.content_size.x;
    let needs_scroll = content_w > viewport_w + 0.5;

    let max_offset = (content_w - viewport_w).max(0.0);
    let mut new_offset = output.state.offset.x;
    if needs_scroll {
        let bar_rect = output.inner_rect;
        let icon_size = 14.0_f32;
        let arrow_area_w = icon_size * 1.6;
        // 스크롤 화살표는 본문 텍스트보다 물러난 톤. 대응 토큰 없음.
        const SCROLL_ARROW_OPACITY: f32 = 0.4;
        let icon_tint: Color32 = ui
            .style()
            .visuals
            .text_color()
            .gamma_multiply(SCROLL_ARROW_OPACITY);

        let left_rect = egui::Rect::from_min_size(
            bar_rect.left_top(),
            egui::vec2(arrow_area_w, bar_rect.height()),
        );
        let right_rect = egui::Rect::from_min_max(
            egui::pos2(bar_rect.right() - arrow_area_w, bar_rect.top()),
            bar_rect.right_bottom(),
        );
        let left_btn = ui.put(
            left_rect,
            egui::Button::image(tasty_icons::CHEVRON_LEFT.image(icon_size, icon_tint))
                .frame(false)
                .min_size(left_rect.size()),
        );
        let right_btn = ui.put(
            right_rect,
            egui::Button::image(tasty_icons::CHEVRON_RIGHT.image(icon_size, icon_tint))
                .frame(false)
                .min_size(right_rect.size()),
        );
        if left_btn.clicked() {
            new_offset = (new_offset - SCROLL_STEP).max(0.0);
        }
        if right_btn.clicked() {
            new_offset = (new_offset + SCROLL_STEP).min(max_offset);
        }
    }
    if (new_offset - output.state.offset.x).abs() > f32::EPSILON {
        let mut s = output.state;
        s.offset.x = new_offset;
        s.store(ui.ctx(), output.id);
        ui.ctx().request_repaint();
    }
}
