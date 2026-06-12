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
    static STATE: RefCell<State> = const { RefCell::new(State { top: 0, sub: 0 }) };
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

fn draw_top_tabs(ui: &mut egui::Ui, _theme: &Theme) {
    let cur_top = STATE.with(|s| s.borrow().top);
    let mut new_top = cur_top;
    let tabs: Vec<(usize, &str)> = TOP_TABS.iter().copied().enumerate().collect();
    ui.horizontal(|ui| {
        tasty_ui_widgets::horizontal_tab_bar_with_arrows(
            ui,
            "layout_2depth_top_scroll",
            &tabs,
            &mut new_top,
        );
    });
    if new_top != cur_top {
        STATE.with(|s| {
            let mut st = s.borrow_mut();
            st.top = new_top;
            st.sub = 0;
        });
    }
}

/// 좌측 sub menu Frame + 우측 콘텐츠. `tasty_ui_widgets::two_depth_layout` 호출.
fn draw_split(ui: &mut egui::Ui, theme: &Theme) {
    let available_height = ui.available_height();
    tasty_ui_widgets::two_depth_layout(
        ui,
        theme,
        available_height,
        |ui| {
            let cur = STATE.with(|s| s.borrow().sub);
            for (idx, label) in SUB_TABS.iter().enumerate() {
                if ui.selectable_label(cur == idx, *label).clicked() {
                    STATE.with(|s| s.borrow_mut().sub = idx);
                }
            }
        },
        |ui| draw_content(ui, theme),
    );
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
            tasty_ui_widgets::tab_content_frame(ui, |ui| {
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
