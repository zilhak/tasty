//! `segmented` — 세그먼트 토글 (디자인 T11 explorer view-mode toggle, design §3.4).
//!
//! 한 컨테이너 안에 상호 배타 선택지를 가로로 묶은 토글. explorer 의 grid/list/detail
//! 전환이 1차 사용처지만, 특정 도메인에 묶이지 않은 **공용 위젯**으로 일반화한다
//! (`labels` 슬라이스 + 현재 선택 index → 새로 클릭된 index).
//!
//! 토큰: 컨테이너 surface-raised + 1px border-strong + radius, 활성 세그먼트
//! accent-primary fill + text-on-accent, 비활성 text-secondary(+hover overlay-hover),
//! 세그먼트 간 1px separator. 색·치수·폰트는 전부 `Theme` 토큰.

use tasty_type_appearance::theme::Theme;

/// 세그먼트 토글. `selected` 가 현재 활성 index, 새로 클릭된 세그먼트 index 를 반환
/// (변경 없으면 `None`). 컨테이너는 자기 콘텐츠 폭만큼만 차지한다.
pub fn segmented(
    ui: &mut egui::Ui,
    theme: &Theme,
    labels: &[&str],
    selected: usize,
) -> Option<usize> {
    let h = theme.item_height_interactive.value();
    let pad_x = theme.spacing_sm.value();
    let radius = theme.corner_radius.value();
    let radius_sm = theme.corner_radius_sm.value();
    let font = egui::FontId::proportional(theme.font_size_body.value());

    let mut clicked = None;
    egui::Frame::new()
        .fill(theme.surface_raised().to_egui())
        .stroke(egui::Stroke::new(
            theme.border_width.value(),
            theme.border_strong().to_egui(),
        ))
        .corner_radius(radius)
        .inner_margin(egui::Margin::ZERO)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
            ui.horizontal(|ui| {
                for (i, label) in labels.iter().enumerate() {
                    let active = i == selected;
                    // 세그먼트 폭 = 텍스트 폭 + 좌우 패딩.
                    let galley = ui.fonts(|f| {
                        f.layout_no_wrap(
                            (*label).to_string(),
                            font.clone(),
                            theme.text_primary().to_egui(),
                        )
                    });
                    let seg_w = galley.size().x + pad_x * 2.0;
                    let (rect, resp) =
                        ui.allocate_exact_size(egui::vec2(seg_w, h), egui::Sense::click());

                    // 세그먼트 간 separator (비활성 인접 경계에만).
                    if i > 0 && !active && i.checked_sub(1) != Some(selected) {
                        ui.painter().vline(
                            rect.left(),
                            rect.y_range(),
                            egui::Stroke::new(
                                theme.border_width.value(),
                                theme.separator.to_egui(),
                            ),
                        );
                    }

                    // 배경: active accent-primary, hover overlay-hover.
                    if active {
                        ui.painter()
                            .rect_filled(rect, radius_sm, theme.accent_primary().to_egui());
                    } else if resp.hovered() {
                        ui.painter().rect_filled(
                            rect,
                            radius_sm,
                            theme.overlay_hover().to_egui_premultiplied(),
                        );
                    }

                    let text_color = if active {
                        theme.text_on_accent().to_egui()
                    } else {
                        theme.text_secondary().to_egui()
                    };
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        label,
                        font.clone(),
                        text_color,
                    );

                    if resp.clicked() && !active {
                        clicked = Some(i);
                    }
                }
            });
        });

    clicked
}
