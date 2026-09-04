//! `layout-shell` specimen — 공용 레이아웃 위젯 3종을 **직접 호출**한다 (Components).
//!
//! 여기 있는 세 함수는 `crates/tasty-ui-widgets` 의 공용 위젯이라 갤러리가 복제하지
//! 않고 **본체와 같은 함수**를 부른다(demo=main —
//! `docs/design/policies/gallery-completeness.md`). 따라서 위젯이 바뀌면 이 specimen
//! 이 자동으로 따라간다.
//!
//! - [`two_depth_layout`] / [`two_depth_layout_filtered`] — 좌측 고정폭 sub-menu
//!   패널(`crust` 배경 + 1px 보더) + 우측 콘텐츠, 사이 8px gap. filtered 변종은
//!   좌측 패널 상단에 섹션 필터 입력 슬롯을 더 얹는다.
//! - [`horizontal_tab_bar_with_arrows`] — 가로 `ScrollArea` 탭 줄. 콘텐츠가
//!   viewport 보다 넓을 때만 좌/우 chevron 이 탭 위에 알파 오버레이로 뜬다(영역을
//!   차지하지 않는다). chevron 클릭은 80px 씩 스크롤.
//! - [`tab_content_frame`] — 탭 콘텐츠를 모달 테두리에서 `TAB_CONTENT_PADDING`
//!   만큼 띄우는 4면 균등 wrapper.
//!
//! **Layouts 페이지의 `twodepth`/`multitab` specimen 과 다른 물건이다.** 그쪽은
//! 디자인 레이아웃 idiom 을 painter 로 전사한 정적 무대이고, 여기는 그 idiom 을
//! 실제로 구현한 **공용 위젯 자체**의 라이브 데모다.

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
        "세 함수 모두 `tasty-ui-widgets` 의 공용 위젯이라 이 specimen 은 복제가 아니라 \
         본체와 같은 호출이다 — 위젯을 고치면 여기도 같이 바뀐다. chevron 은 콘텐츠가 \
         viewport 보다 넓을 때만 나타나므로, 좁은 폭으로 묶어야 그 상태를 볼 수 있다. \
         필터 입력은 위젯이 그리지만 어떤 항목을 남길지는 호출자가 정한다.",
    );
}
