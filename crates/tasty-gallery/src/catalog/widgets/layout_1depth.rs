//! Layout — 1depth (Plugins 창 idiom).
//!
//! 본체 `src/view/plugins/ui.rs::draw_plugins_panel` 의 *상단 헤더 + 1단 탭 + 좌측 리스트 + 우측 디테일* 패턴 재현.
//! - TopBottomPanel: 제목 + 1단 탭 (List / Add 등)
//! - List 탭: SidePanel(좌측 리스트) + CentralPanel(우측 디테일)
//! - Add 탭: 단일 form
//!
//! Theme 만 의존. 본체 binary 미의존.

use std::cell::RefCell;

use tasty_type_appearance::theme::Theme;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    List,
    Add,
}

struct State {
    tab: Tab,
    selected: usize,
}

thread_local! {
    static STATE: RefCell<State> = const {
        RefCell::new(State {
            tab: Tab::List,
            selected: 0,
        })
    };
}

const LIST_ITEMS: &[(&str, &str)] = &[
    ("Menu1", "enabled"),
    ("Menu2", "enabled"),
    ("Menu3", "disabled"),
    ("Menu4", "enabled"),
    ("Menu5", "enabled"),
    ("Menu6", "enabled"),
    ("Menu7", "enabled"),
    ("Menu8", "disabled"),
];

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    ui.label(
        egui::RichText::new(
            "본체 src/view/plugins/ui.rs 의 *상단 헤더 + 1단 탭 + 좌측 리스트 + 우측 디테일* 패턴.",
        )
        .color(egui::Color32::from(theme.subtext0))
        .size(11.0),
    );
    ui.add_space(8.0);

    let panel_h = 360.0;
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.set_min_size(egui::vec2(ui.available_width(), panel_h));
        ui.set_max_height(panel_h);

        draw_header(ui, theme);
        ui.separator();

        let tab = STATE.with(|s| s.borrow().tab);
        match tab {
            Tab::List => draw_list_tab(ui, theme),
            Tab::Add => draw_add_tab(ui, theme),
        }
    });
}

fn draw_header(ui: &mut egui::Ui, theme: &Theme) {
    ui.horizontal(|ui| {
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new("Plugins")
                .color(egui::Color32::from(theme.text))
                .size(14.0),
        );
    });
    ui.horizontal(|ui| {
        ui.add_space(4.0);
        let cur = STATE.with(|s| s.borrow().tab);
        for (tab, label) in [(Tab::List, "Installed"), (Tab::Add, "Add Plugin")] {
            if ui.selectable_label(cur == tab, label).clicked() {
                STATE.with(|s| s.borrow_mut().tab = tab);
            }
        }
    });
}

fn draw_list_tab(ui: &mut egui::Ui, theme: &Theme) {
    // 좌측 리스트 + 우측 디테일을 horizontal 로 분할.
    let avail = ui.available_size();
    let list_w = (avail.x * 0.32).clamp(160.0, 260.0);
    ui.horizontal(|ui| {
        // 좌측 리스트
        ui.allocate_ui_with_layout(
            egui::vec2(list_w, avail.y),
            egui::Layout::top_down(egui::Align::LEFT),
            |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("layout_1depth_list")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        egui::Frame::new()
                            .inner_margin(egui::Margin::same(8))
                            .show(ui, |ui| {
                                ui.set_min_width(list_w - 8.0);
                                let cur = STATE.with(|s| s.borrow().selected);
                                for (idx, (name, status)) in LIST_ITEMS.iter().enumerate() {
                                    let label = format!("{name}   ·   {status}");
                                    if ui.selectable_label(cur == idx, label).clicked() {
                                        STATE.with(|s| s.borrow_mut().selected = idx);
                                    }
                                }
                            });
                    });
            },
        );
        ui.separator();
        // 우측 디테일
        ui.allocate_ui_with_layout(
            egui::vec2(avail.x - list_w - 16.0, avail.y),
            egui::Layout::top_down(egui::Align::LEFT),
            |ui| {
                egui::Frame::new()
                    .inner_margin(egui::Margin::same(8))
                    .show(ui, |ui| {
                        let cur = STATE.with(|s| s.borrow().selected);
                        let (name, status) = LIST_ITEMS.get(cur).copied().unwrap_or(("?", "?"));
                        ui.label(
                            egui::RichText::new(name)
                                .color(egui::Color32::from(theme.text))
                                .heading(),
                        );
                        ui.label(
                            egui::RichText::new(format!("Status: {status}"))
                                .color(egui::Color32::from(theme.subtext0))
                                .size(11.0),
                        );
                        ui.add_space(8.0);
                        ui.label(
                            egui::RichText::new(
                                "(여기에 권한, surface_kinds, 로그 경로 등 디테일)",
                            )
                            .color(egui::Color32::from(theme.subtext1))
                            .size(11.0),
                        );
                    });
            },
        );
    });
}

fn draw_add_tab(ui: &mut egui::Ui, theme: &Theme) {
    egui::Frame::new()
        .inner_margin(egui::Margin::same(8))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new("Plugin folder path")
                    .color(egui::Color32::from(theme.subtext0))
                    .size(11.0),
            );
            let mut buf = String::new();
            ui.add(
                egui::TextEdit::singleline(&mut buf)
                    .desired_width(ui.available_width() - 8.0)
                    .hint_text("/path/to/plugin"),
            );
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                // mock: click 결과 무시 — 갤러리 시각 검증 전용.
                let _browse = ui.button("Browse…");
                let _confirm = ui.button("Confirm path");
            });
            ui.add_space(12.0);
            ui.label(
                egui::RichText::new("(여기에 매니페스트 프리뷰, 신뢰 경고, 추가 버튼)")
                    .color(egui::Color32::from(theme.subtext1))
                    .size(11.0),
            );
        });
}
