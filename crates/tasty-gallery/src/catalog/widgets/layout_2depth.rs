//! Layout — 2depth (Settings 창 idiom).
//!
//! 본체 `src/view/settings/ui.rs::draw_settings_panel` 의 *상단 탭 + sub tab + 콘텐츠 + 하단 Save/Cancel* 패턴 재현.
//! - 상단: 가로 스크롤 가능 탭바 + overlay chevron 화살표 (스크롤 필요시)
//! - 중간: 활성 탭의 sub tab 바 (해당 탭이 sub 를 가질 때만)
//! - 콘텐츠: 분기된 form 영역
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
    "General",
    "Terminal",
    "Appearance",
    "Clipboard",
    "Notifications",
    "Keybindings",
    "Accessibility",
    "File Handler",
    "Updates",
];

// 활성 탭의 sub tab (없으면 빈 슬라이스).
fn sub_tabs_for(top: usize) -> &'static [&'static str] {
    match TOP_TABS.get(top).copied() {
        Some("Appearance") => &["Tasty", "Theme", "Editor"],
        Some("File Handler") => &["Detectors", "Handlers", "Extension Mapping"],
        _ => &[],
    }
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    ui.label(
        egui::RichText::new("Layout — 2 depth (Settings idiom)")
            .color(egui::Color32::from(theme.text))
            .heading(),
    );
    ui.label(
        egui::RichText::new(
            "본체 src/view/settings/ui.rs 의 *상단 탭 + sub tab + content + 하단 Save/Cancel* 패턴. \
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
                if !sub_tabs_for(STATE.with(|s| s.borrow().top)).is_empty() {
                    draw_sub_tabs(ui);
                    ui.separator();
                }
                ui.add_space(4.0);
                draw_content(ui, theme);
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

fn draw_sub_tabs(ui: &mut egui::Ui) {
    let (top, sub) = STATE.with(|s| {
        let st = s.borrow();
        (st.top, st.sub)
    });
    let tabs = sub_tabs_for(top);
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        for (idx, label) in tabs.iter().enumerate() {
            if ui.selectable_label(sub == idx, *label).clicked() {
                STATE.with(|s| s.borrow_mut().sub = idx);
            }
        }
    });
}

fn draw_content(ui: &mut egui::Ui, theme: &Theme) {
    let (top, sub) = STATE.with(|s| {
        let st = s.borrow();
        (st.top, st.sub)
    });
    let top_label = TOP_TABS.get(top).copied().unwrap_or("?");
    let sub_tabs = sub_tabs_for(top);
    let path = if sub_tabs.is_empty() {
        top_label.to_string()
    } else {
        format!(
            "{top_label} / {}",
            sub_tabs.get(sub).copied().unwrap_or("?")
        )
    };
    egui::ScrollArea::vertical()
        .id_salt("layout_2depth_content_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(format!("Content: {path}"))
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
}

fn draw_bottom_buttons(ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.add_space(ui.available_width() - 160.0);
        // mock: click 결과 무시 — 갤러리 시각 검증 전용.
        let _cancel = ui.button("Cancel");
        let _save = ui.button("Save");
    });
}
