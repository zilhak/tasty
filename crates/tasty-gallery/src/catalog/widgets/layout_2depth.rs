//! Layout — 2depth (Settings → Keybindings 탭 idiom).
//!
//! 본체 `src/view/settings/ui/keybindings_tab.rs::draw_keybindings_tab` 의
//! *상단 1depth 탭 + 좌측 2depth 메뉴 + 우측 콘텐츠 + 하단 Save/Cancel* 패턴 재현.
//!
//! - 상단: 가로 스크롤 가능 탭바 + overlay chevron 화살표 (스크롤 필요시)
//! - 좌측: 고정 폭 (100px) `Frame` 패널 — `vertical` `selectable_label` 리스트
//! - 우측: 활성 (top, sub) 조합에 따른 콘텐츠
//! - 하단: Save / Cancel 버튼 영역
//!
//! Theme 만 의존. 본체 binary 미의존.

use std::cell::RefCell;

use tasty_type_appearance::theme::Theme;

struct State {
    top: usize,
    sub: usize,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State { top: 0, sub: 0 });
}

const TOP_TABS: &[&str] = &[
    "Tab1", "Tab2", "Tab3", "Tab4", "Tab5", "Tab6", "Tab7", "Tab8", "Tab9",
];

// 모든 top tab 이 좌측 sub menu 를 가진다 (단축키 탭 idiom — 항상 좌측 패널 노출).
const SUB_TABS: &[&str] = &["Sub1", "Sub2", "Sub3", "Sub4", "Sub5"];

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    ui.label(
        egui::RichText::new(
            "본체 src/view/settings/ui/keybindings_tab.rs 의 \
             *상단 1depth 탭 + 좌측 2depth 메뉴 + 우측 콘텐츠 + 하단 Save/Cancel* 패턴. \
             상단 탭은 가로 ScrollArea + overlay chevron (스크롤 필요시).",
        )
        .color(egui::Color32::from(theme.subtext0))
        .size(11.0),
    );
    ui.add_space(8.0);

    let panel_h = 420.0;
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.set_min_size(egui::vec2(ui.available_width(), panel_h));
        ui.set_max_height(panel_h);

        let avail_h = ui.available_height();
        let bottom_h = 36.0;
        let content_h = (avail_h - bottom_h).max(0.0);

        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), content_h),
            egui::Layout::top_down(egui::Align::LEFT),
            |ui| {
                draw_top_tabs(ui, theme);
                ui.separator();
                ui.add_space(4.0);
                draw_split(ui, theme);
            },
        );

        ui.separator();
        draw_bottom_buttons(ui);
    });
}

fn draw_top_tabs(ui: &mut egui::Ui, theme: &Theme) {
    const SCROLL_STEP: f32 = 80.0;
    ui.horizontal(|ui| {
        let output = egui::ScrollArea::horizontal()
            .id_salt("layout_2depth_top_scroll")
            .auto_shrink([false, true])
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .max_width(ui.available_width())
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let cur = STATE.with(|s| s.borrow().top);
                    for (idx, label) in TOP_TABS.iter().enumerate() {
                        if ui.selectable_label(cur == idx, *label).clicked() {
                            STATE.with(|s| {
                                let mut st = s.borrow_mut();
                                st.top = idx;
                                st.sub = 0;
                            });
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
            let icon_tint = egui::Color32::from(theme.text).gamma_multiply(0.4);

            let left_rect = egui::Rect::from_min_size(
                bar_rect.left_top(),
                egui::vec2(arrow_area_w, bar_rect.height()),
            );
            let right_rect = egui::Rect::from_min_max(
                egui::pos2(bar_rect.right() - arrow_area_w, bar_rect.top()),
                bar_rect.right_bottom(),
            );
            let l = ui.put(
                left_rect,
                egui::Button::image(
                    egui::Image::new(egui::include_image!(
                        "../../../../../assets/icons/chevron-left.svg"
                    ))
                    .tint(icon_tint)
                    .fit_to_exact_size(egui::vec2(icon_size, icon_size)),
                )
                .frame(false)
                .min_size(left_rect.size()),
            );
            let r = ui.put(
                right_rect,
                egui::Button::image(
                    egui::Image::new(egui::include_image!(
                        "../../../../../assets/icons/chevron-right.svg"
                    ))
                    .tint(icon_tint)
                    .fit_to_exact_size(egui::vec2(icon_size, icon_size)),
                )
                .frame(false)
                .min_size(right_rect.size()),
            );
            if l.clicked() {
                new_offset = (new_offset - SCROLL_STEP).max(0.0);
            }
            if r.clicked() {
                new_offset = (new_offset + SCROLL_STEP).min(max_offset);
            }
        }
        if (new_offset - output.state.offset.x).abs() > f32::EPSILON {
            let mut s = output.state;
            s.offset.x = new_offset;
            s.store(ui.ctx(), output.id);
            ui.ctx().request_repaint();
        }
    });
}

/// 좌측 sub menu Frame + 우측 콘텐츠. 단축키 탭 idiom 그대로.
fn draw_split(ui: &mut egui::Ui, theme: &Theme) {
    let available_height = ui.available_height();
    ui.horizontal_top(|ui| {
        // 좌측 sub menu (고정 폭 100, 단축키 탭과 동일)
        egui::Frame::new()
            .fill(egui::Color32::from(theme.crust))
            .stroke(egui::Stroke::new(1.0, egui::Color32::from(theme.surface0)))
            .corner_radius(4.0)
            .inner_margin(egui::Margin::symmetric(6, 6))
            .show(ui, |ui| {
                ui.set_width(150.0);
                ui.set_min_height(available_height);

                ui.vertical(|ui| {
                    let cur = STATE.with(|s| s.borrow().sub);
                    for (idx, label) in SUB_TABS.iter().enumerate() {
                        if ui.selectable_label(cur == idx, *label).clicked() {
                            STATE.with(|s| s.borrow_mut().sub = idx);
                        }
                    }
                });
            });

        ui.add_space(8.0);

        // 우측 콘텐츠
        ui.vertical(|ui| {
            ui.set_max_height(available_height);
            draw_content(ui, theme);
        });
    });
}

fn draw_content(ui: &mut egui::Ui, theme: &Theme) {
    let (top, sub) = STATE.with(|s| {
        let st = s.borrow();
        (st.top, st.sub)
    });
    let top_label = TOP_TABS.get(top).copied().unwrap_or("?");
    let sub_label = SUB_TABS.get(sub).copied().unwrap_or("?");
    egui::ScrollArea::vertical()
        .id_salt("layout_2depth_content_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Frame::new()
                .inner_margin(egui::Margin::same(8))
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(format!("Content: {top_label} / {sub_label}"))
                            .color(egui::Color32::from(theme.text))
                            .size(12.0),
                    );
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new("(여기에 라벨 + 입력 위젯 grid)")
                            .color(egui::Color32::from(theme.subtext0))
                            .size(11.0),
                    );
                    ui.add_space(40.0);
                    // 스크롤 데모용 더미 행
                    for i in 0..30 {
                        ui.label(
                            egui::RichText::new(format!("dummy row #{i}"))
                                .color(egui::Color32::from(theme.subtext1))
                                .size(11.0),
                        );
                    }
                });
        });
}

fn draw_bottom_buttons(ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.add_space(ui.available_width() - 160.0);
        // mock: click 결과 무시 — 갤러리 시각 검증 전용.
        let _cancel = ui.button("Cancel");
        let _save = ui.button("Save");
    });
}
