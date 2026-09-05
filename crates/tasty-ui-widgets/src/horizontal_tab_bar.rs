//! Horizontal tab bar with overflow chevron arrows.
//!
//! **현재 소비처는 갤러리 primitive specimen `components/prim_layout_shell.rs` 하나뿐이다.**
//! 본체 settings 의 L1 탭 밴드(`src/view/settings/ui.rs` `draw_l1_tab_band`)는 이 위젯을
//! 부르지 않고 자기 `Frame` 으로 밴드를 그린다 — 좌측 타이틀·세로 구분선이 같은 줄에
//! 들어가야 해서 탭만 담는 이 컨테이너에 맞지 않는다.
//!
//! - 가로 `ScrollArea` 안에 `selectable_label` 리스트.
//! - 콘텐츠 폭 > viewport 폭 일 때만 좌/우 chevron overlay 표시.
//!   영역을 차지하지 않고 알파 0.4 로 탭 위에 그려짐.
//! - chevron 클릭 시 `SCROLL_STEP` 만큼 offset 이동.
//!
//! `id_salt` 는 호출자 별로 unique 해야 함 (ScrollArea state 분리).
//! chevron 아이콘은 canonical `tasty_icons::CHEVRON_LEFT`/`CHEVRON_RIGHT`
//! (2px stroke) 를 `Icon::image` 로 렌더. 호출자는 `egui_extras::install_image_loaders`
//! 가 미리 호출됐다고 가정 (본체·갤러리 모두 이미 호출 중).

use egui::Color32;

/// 한 step 스크롤 거리 (평균 탭 너비 ~80px 기준).
const SCROLL_STEP: f32 = 80.0;

/// 가로 탭 바 + 필요 시 좌/우 chevron overlay. 클릭 시 `*active = *tab`.
///
/// 시각: `egui::ScrollArea::horizontal` + `selectable_label` 반복. 콘텐츠가
/// viewport 보다 크면 좌/우 끝 14px 폭 영역에 chevron 아이콘 overlay (탭 위에
/// 떠 있음, 알파 0.4) — 클릭 시 80px 스크롤.
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
