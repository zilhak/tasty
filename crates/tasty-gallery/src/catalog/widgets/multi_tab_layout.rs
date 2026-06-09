//! Multi-tier tab layout 데모 (Tier 2 widget).
//!
//! 같은 *가로 축* 에 탭이 N단 쌓일 때 depth 별로 시각 위계를 부여하는 패턴.
//!
//! - **1단 (top)**: 일반 `selectable_label` + 아래 `ui.separator()` (밑줄).
//! - **2단 (sub)**: 외곽 `Frame` 으로 묶은 *segmented control* — Frame 이
//!   "이 탭들은 한 그룹" 임을 시각적으로 표명. 활성 항목은 `selectable_label`
//!   기본 selection 색으로 강조.
//! - **3단 (subsub)**: 같은 segmented 패턴을 *작은 사이즈* 로 한 번 더.
//!   글씨 11 + 더 얇은 inner padding + 더 작은 corner.
//!
//! `Theme` 만 의존. 본체 binary 미의존.

use std::cell::RefCell;

use tasty_type_appearance::theme::Theme;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Case {
    SingleTier,
    TwoTier,
    ThreeTier,
    ManyTopTabs,
}

struct DemoState {
    case: Case,
    top: usize,
    sub: usize,
    subsub: usize,
}

thread_local! {
    static STATE: RefCell<DemoState> = RefCell::new(DemoState {
        case: Case::TwoTier,
        top: 0,
        sub: 0,
        subsub: 0,
    });
}

const TOP_TABS_FEW: &[&str] = &["General", "Terminal", "Appearance", "Plugins"];

const TOP_TABS_MANY: &[&str] = &[
    "General",
    "Terminal",
    "Appearance",
    "Clipboard",
    "Notifications",
    "Keybindings",
    "Performance",
    "Accessibility",
    "File Handler",
    "Updates",
];

const SUB_TABS: &[&str] = &["Tasty", "Editor", "Cursor"];
const SUBSUB_TABS: &[&str] = &["Light", "Dark", "Auto"];

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    ui.label(
        egui::RichText::new(
            "같은 가로 축에서 depth 를 시각 위계로 표현. \
             1단: 일반 탭 + 밑줄. 2단: 외곽 Frame 으로 묶은 segmented control. \
             3단: 같은 segmented 패턴 + 작은 사이즈.",
        )
        .color(egui::Color32::from(theme.subtext0))
        .size(11.0),
    );
    ui.add_space(8.0);

    // Case 선택 (데모 자체)
    let mut case = STATE.with(|s| s.borrow().case);
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("Case:")
                .color(egui::Color32::from(theme.subtext0))
                .strong(),
        );
        for (label, value) in [
            ("1 단", Case::SingleTier),
            ("2 단", Case::TwoTier),
            ("3 단", Case::ThreeTier),
            ("스크롤 필요 (탭 많음)", Case::ManyTopTabs),
        ] {
            if ui.selectable_label(case == value, label).clicked() {
                case = value;
                STATE.with(|s| {
                    let mut st = s.borrow_mut();
                    st.case = value;
                    st.top = 0;
                    st.sub = 0;
                    st.subsub = 0;
                });
            }
        }
    });
    ui.add_space(8.0);

    // 본 데모 패널 — 본체 settings 의 modal 상단 영역과 같은 시각 idiom.
    let panel_h = match case {
        Case::SingleTier => 90.0,
        Case::TwoTier => 150.0,
        Case::ThreeTier => 210.0,
        Case::ManyTopTabs => 200.0,
    };
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.set_min_size(egui::vec2(ui.available_width(), panel_h));
        ui.set_max_height(panel_h);

        let top_tabs = if case == Case::ManyTopTabs {
            TOP_TABS_MANY
        } else {
            TOP_TABS_FEW
        };

        // 1단: 일반 탭 + 밑줄 separator.
        draw_top_tab_bar(ui, theme, top_tabs);
        ui.separator();

        if matches!(case, Case::TwoTier | Case::ThreeTier) {
            ui.add_space(6.0);
            // 2단: segmented control.
            draw_sub_segmented(ui, theme, SUB_TABS, |idx| {
                STATE.with(|s| s.borrow_mut().sub = idx);
            });
        }
        if matches!(case, Case::ThreeTier) {
            ui.add_space(4.0);
            // 3단: segmented control (작은 사이즈).
            draw_subsub_segmented(ui, theme, SUBSUB_TABS);
        }

        ui.add_space(8.0);
        draw_content(ui, theme, case);
    });
}

fn draw_top_tab_bar(ui: &mut egui::Ui, theme: &Theme, tabs: &[&str]) {
    const SCROLL_STEP: f32 = 80.0;
    ui.horizontal(|ui| {
        let output = egui::ScrollArea::horizontal()
            .id_salt("demo_top_scroll")
            .auto_shrink([false, true])
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .max_width(ui.available_width())
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let cur = STATE.with(|s| s.borrow().top);
                    for (idx, label) in tabs.iter().enumerate() {
                        if ui.selectable_label(cur == idx, *label).clicked() {
                            STATE.with(|s| s.borrow_mut().top = idx);
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

/// 2단: 외곽 Frame 으로 묶은 segmented control. *이 탭들은 한 그룹* 임을 시각화.
fn draw_sub_segmented(ui: &mut egui::Ui, theme: &Theme, tabs: &[&str], on_click: impl Fn(usize)) {
    egui::Frame::new()
        .fill(egui::Color32::from(theme.mantle))
        .stroke(egui::Stroke::new(1.0, egui::Color32::from(theme.surface0)))
        .corner_radius(6.0)
        .inner_margin(egui::Margin::symmetric(4, 3))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let cur = STATE.with(|s| s.borrow().sub);
                for (idx, label) in tabs.iter().enumerate() {
                    if ui.selectable_label(cur == idx, *label).clicked() {
                        on_click(idx);
                    }
                }
            });
        });
}

/// 3단: 같은 segmented 패턴, 글씨 11 + 더 얇은 inner padding + 작은 corner.
fn draw_subsub_segmented(ui: &mut egui::Ui, theme: &Theme, tabs: &[&str]) {
    egui::Frame::new()
        .fill(egui::Color32::from(theme.mantle))
        .stroke(egui::Stroke::new(1.0, egui::Color32::from(theme.surface0)))
        .corner_radius(4.0)
        .inner_margin(egui::Margin::symmetric(3, 2))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let cur = STATE.with(|s| s.borrow().subsub);
                for (idx, label) in tabs.iter().enumerate() {
                    let rt = egui::RichText::new(*label)
                        .size(11.0)
                        .color(egui::Color32::from(theme.text));
                    if ui.selectable_label(cur == idx, rt).clicked() {
                        STATE.with(|s| s.borrow_mut().subsub = idx);
                    }
                }
            });
        });
}

fn draw_content(ui: &mut egui::Ui, theme: &Theme, case: Case) {
    let (top, sub, subsub) = STATE.with(|s| {
        let st = s.borrow();
        (st.top, st.sub, st.subsub)
    });
    let top_label = if case == Case::ManyTopTabs {
        TOP_TABS_MANY.get(top).copied().unwrap_or("?")
    } else {
        TOP_TABS_FEW.get(top).copied().unwrap_or("?")
    };
    let sub_label = SUB_TABS.get(sub).copied().unwrap_or("?");
    let subsub_label = SUBSUB_TABS.get(subsub).copied().unwrap_or("?");
    let path = match case {
        Case::SingleTier => top_label.to_string(),
        Case::TwoTier => format!("{top_label} / {sub_label}"),
        Case::ThreeTier => format!("{top_label} / {sub_label} / {subsub_label}"),
        Case::ManyTopTabs => top_label.to_string(),
    };
    ui.label(
        egui::RichText::new(format!("Content: {path}"))
            .color(egui::Color32::from(theme.text))
            .size(12.0),
    );
    ui.label(
        egui::RichText::new("(여기에 분기된 폼/리스트/그리드를 그린다)")
            .color(egui::Color32::from(theme.subtext0))
            .size(11.0),
    );
}
