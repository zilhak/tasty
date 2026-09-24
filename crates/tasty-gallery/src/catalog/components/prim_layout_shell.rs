//! 공용 레이아웃 위젯의 동작 예제. 본체와 같은 함수를 호출한다.
//! Layouts 페이지의 정적 화면 예제와 달리 탭 이동·스크롤·필터 입력을 조작할 수 있다.

use std::cell::RefCell;

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{
    horizontal_tab_bar_with_arrows, tab_content_frame, two_depth_layout, two_depth_layout_filtered,
};

use crate::catalog::spec::{self, StageVariant, TokenChip};

#[derive(Clone, Copy, PartialEq, Eq)]
enum DemoTab {
    General,
    Terminal,
    Appearance,
    Keybindings,
    Handler,
    Misc,
    Plugins,
}

struct State {
    tab: DemoTab,
    section: usize,
    filter: String,
}

thread_local! {
    static STATE: RefCell<State> = const {
        RefCell::new(State {
            tab: DemoTab::Appearance,
            section: 0,
            filter: String::new(),
        })
    };
}

const TABS: &[(DemoTab, &str)] = &[
    (DemoTab::General, "General"),
    (DemoTab::Terminal, "Terminal"),
    (DemoTab::Appearance, "Appearance"),
    (DemoTab::Keybindings, "Keybindings"),
    (DemoTab::Handler, "Handler"),
    (DemoTab::Misc, "Misc"),
    (DemoTab::Plugins, "Plugins"),
];

const SECTIONS: &[&str] = &["Theme", "Colors", "General", "Display", "Terminal"];

/// 우측 콘텐츠 — `tab_content_frame` 안에 그려 padding idiom 까지 함께 보인다.
fn content(ui: &mut egui::Ui, theme: &Theme, section: &str) {
    tab_content_frame(ui, |ui| {
        ui.label(
            egui::RichText::new(section)
                .size(theme.font_size_max.value())
                .strong()
                .color(theme.text_primary().to_egui()),
        );
        ui.add_space(theme.spacing_sm.value());
        ui.label(
            egui::RichText::new(
                "tab_content_frame wraps this column so the content never touches the modal edge.",
            )
            .size(theme.font_size_body.value())
            .color(theme.text_secondary().to_egui()),
        );
    });
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    let body_h = theme.spacing_xl.value() * 6.0;

    spec::stage(ui, theme, StageVariant::Column, |ui| {
        spec::cluster(ui, theme, "horizontal_tab_bar_with_arrows", |ui| {
            ui.vertical(|ui| {
                // 좁은 폭으로 묶어 overflow chevron 이 실제로 뜨는 상태를 보여준다.
                ui.set_max_width(theme.measure_sm.value());
                STATE.with(|s| {
                    let st = &mut *s.borrow_mut();
                    horizontal_tab_bar_with_arrows(ui, "gallery_layout_shell", TABS, &mut st.tab);
                });
            });
        });

        spec::cluster(ui, theme, "two_depth_layout", |ui| {
            ui.vertical(|ui| {
                ui.set_max_width(theme.measure_lg.value());
                STATE.with(|s| {
                    let st = &mut *s.borrow_mut();
                    let section = st.section;
                    let mut picked = None;
                    two_depth_layout(
                        ui,
                        theme,
                        body_h,
                        |ui| {
                            for (i, name) in SECTIONS.iter().enumerate() {
                                if ui.selectable_label(section == i, *name).clicked() {
                                    picked = Some(i);
                                }
                            }
                        },
                        |ui| content(ui, theme, SECTIONS[section]),
                    );
                    if let Some(i) = picked {
                        st.section = i;
                    }
                });
            });
        });

        spec::cluster(ui, theme, "two_depth_layout_filtered", |ui| {
            ui.vertical(|ui| {
                ui.set_max_width(theme.measure_lg.value());
                STATE.with(|s| {
                    let st = &mut *s.borrow_mut();
                    let needle = st.filter.trim().to_lowercase();
                    let section = st.section;
                    let mut picked = None;
                    two_depth_layout_filtered(
                        ui,
                        theme,
                        body_h,
                        &mut st.filter,
                        "Filter sections…",
                        |ui| {
                            for (i, name) in SECTIONS.iter().enumerate() {
                                if !needle.is_empty() && !name.to_lowercase().contains(&needle) {
                                    continue;
                                }
                                if ui.selectable_label(section == i, *name).clicked() {
                                    picked = Some(i);
                                }
                            }
                        },
                        |ui| content(ui, theme, SECTIONS[section]),
                    );
                    if let Some(i) = picked {
                        st.section = i;
                    }
                });
            });
        });
    });

    spec::meta(
        ui,
        theme,
        &[
            (
                "two-depth",
                "좌 고정폭 패널(crust + 1px) · gap 8 · 우 콘텐츠",
            ),
            ("filtered", "좌측 패널 상단에 필터 입력 슬롯 추가"),
            (
                "tab bar",
                "가로 ScrollArea · overflow 시에만 chevron 오버레이(알파)",
            ),
            ("scroll step", "chevron 1클릭 = 80px"),
            ("tab content", "4면 균등 TAB_CONTENT_PADDING wrapper"),
        ],
        &[
            TokenChip::new("bg-app", "left panel", theme.bg_app().to_egui()),
            TokenChip::new(
                "border-default",
                "panel border",
                theme.border_default().to_egui(),
            ),
            TokenChip::new(
                "text-secondary",
                "content prose",
                theme.text_secondary().to_egui(),
            ),
        ],
    );

    spec::note(
        ui,
        theme,
        "공용 레이아웃 위젯을 본체와 같이 호출한다. 탭이 가용 폭을 넘으면 스크롤 화살표가 나타난다. 필터 입력은 위젯이 그리며 어떤 항목을 남길지는 호출자가 정한다.",
    );
}
