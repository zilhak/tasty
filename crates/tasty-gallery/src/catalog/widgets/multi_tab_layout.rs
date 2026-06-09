//! Multi-tier tab layout 데모 (Tier 2 widget).
//!
//! 본체의 *Settings modal* / *Layout Preset window* 가 사용하는 다단 탭 레이아웃
//! 패턴 (`Top tab bar → Sub tab bar → Content`) 의 시각 모방.
//! 본체 의존 없음 — Theme + 더미 데이터로 동일 idiom 재현.
//!
//! ## 본체 idiom (Settings 기준 — `src/view/settings/ui.rs`)
//! - Top tab bar: 가로 `ScrollArea` + `selectable_label` 리스트 + 콘텐츠 width
//!   가 viewport 보다 클 때만 *overlay* 좌우 화살표 (alpha 0.4, 글자 크기)
//! - Sub tab bar: `ui.horizontal` + `selectable_label` (보통 짧아 스크롤 불필요)
//! - Content: `ui.separator()` + 분기 draw
//!
//! 본 데모는 시각만 보여주므로 클릭 인터랙션은 `thread_local` 상태에 저장.

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
            "본체 Settings / Layout Preset 의 다단 탭 레이아웃을 동일 idiom 으로 재현. \
             상단: 가로 ScrollArea + overlay 화살표 (스크롤 필요시만, alpha 0.4). \
             중간: ui.horizontal + selectable_label. 하단: separator + content.",
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
        Case::SingleTier => 80.0,
        Case::TwoTier => 130.0,
        Case::ThreeTier => 180.0,
        Case::ManyTopTabs => 180.0,
    };
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.set_min_size(egui::vec2(ui.available_width(), panel_h));
        ui.set_max_height(panel_h);

        let top_tabs = if case == Case::ManyTopTabs {
            TOP_TABS_MANY
        } else {
            TOP_TABS_FEW
        };

        draw_top_tab_bar(ui, theme, top_tabs);

        if matches!(case, Case::TwoTier | Case::ThreeTier) {
            ui.add_space(4.0);
            draw_sub_tab_bar(ui, theme, SUB_TABS, |idx| {
                STATE.with(|s| s.borrow_mut().sub = idx);
            });
        }
        if matches!(case, Case::ThreeTier) {
            draw_sub_sub_tab_bar(ui, theme, SUBSUB_TABS);
        }

        ui.separator();
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

fn draw_sub_tab_bar(ui: &mut egui::Ui, _theme: &Theme, tabs: &[&str], on_click: impl Fn(usize)) {
    ui.horizontal(|ui| {
        let cur = STATE.with(|s| s.borrow().sub);
        for (idx, label) in tabs.iter().enumerate() {
            if ui.selectable_label(cur == idx, *label).clicked() {
                on_click(idx);
            }
        }
    });
}

fn draw_sub_sub_tab_bar(ui: &mut egui::Ui, theme: &Theme, tabs: &[&str]) {
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new("▸")
                .color(egui::Color32::from(theme.subtext0))
                .size(11.0),
        );
        let cur = STATE.with(|s| s.borrow().subsub);
        for (idx, label) in tabs.iter().enumerate() {
            let resp = ui.selectable_label(
                cur == idx,
                egui::RichText::new(*label)
                    .size(11.0)
                    .color(egui::Color32::from(theme.text)),
            );
            if resp.clicked() {
                STATE.with(|s| s.borrow_mut().subsub = idx);
            }
        }
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
