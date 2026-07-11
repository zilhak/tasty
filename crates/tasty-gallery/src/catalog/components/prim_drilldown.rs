//! DrillDown primitive specimen — 디자인 `components/navigation/DrillDown` 카드.
//!
//! master→detail content-swap: 풀폭 ListCtrl(프리셋 목록) → 항목 선택 시 영역
//! 전체가 디테일(프리뷰 + back bar 의 Apply 액션)로 교체, ← 로 복귀. 디자인
//! `DrillDown.prompt.md` 의 canonical 예제(Settings › Keybindings › Preset)
//! 그대로. 전환은 즉시(0ms) — opt-in animate 는 전사하지 않는다.

use std::cell::RefCell;

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{
    Button, ButtonVariant, ControlSize, DrillDown, DrillDownView, ListCtrl, ListCtrlItem,
    TagVariant, kbd, tag,
};

use crate::catalog::spec::{StageVariant, TokenChip, meta, stage};

thread_local! {
    static VIEW: RefCell<DrillDownView> = const { RefCell::new(DrillDownView::List) };
    static SEL: RefCell<usize> = const { RefCell::new(0) };
}

/// 데모 프리셋 — 이름 + 설명 + 프리뷰 바인딩(동작, 키).
struct Preset {
    name: &'static str,
    desc: &'static str,
    bindings: [(&'static str, &'static str); 3],
}

const PRESETS: [Preset; 3] = [
    Preset {
        name: "Default",
        desc: "Tasty stock bindings",
        bindings: [
            ("New tab", "Ctrl+T"),
            ("Split pane", "Ctrl+D"),
            ("Close surface", "Ctrl+W"),
        ],
    },
    Preset {
        name: "Mac",
        desc: "⌘-based, TextEdit-style",
        bindings: [
            ("New tab", "⌘T"),
            ("Split pane", "⌘D"),
            ("Close surface", "⌘W"),
        ],
    },
    Preset {
        name: "Vim",
        desc: "modal, hjkl motions",
        bindings: [
            ("New tab", ":tabnew"),
            ("Split pane", ":vsp"),
            ("Close surface", ":q"),
        ],
    },
];

/// DrillDown — list ⇄ detail 교체 · back bar(← + 제목 + Apply) · 내부 스크롤.
pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    stage(ui, theme, StageVariant::Tight, |ui| {
        egui::Frame::new()
            .fill(egui::Color32::from(theme.bg_panel()))
            .stroke(egui::Stroke::new(
                theme.border_width.value(),
                egui::Color32::from(theme.border_default()),
            ))
            .corner_radius(theme.corner_radius.value())
            .show(ui, |ui| {
                ui.set_width(theme.measure_md.value());
                let view = VIEW.with(|v| *v.borrow());
                let sel = SEL.with(|s| *s.borrow());
                let preset = &PRESETS[sel];

                let apply = |ui: &mut egui::Ui, th: &Theme| {
                    Button::new("Apply")
                        .variant(ButtonVariant::Primary)
                        .size(ControlSize::Sm)
                        .show(ui, th);
                };
                let out = DrillDown::new("prim_drilldown")
                    .view(view)
                    .title(preset.name)
                    .back_label("Back")
                    .height(theme.measure_sm.value())
                    .show(
                        ui,
                        theme,
                        |ui, th| {
                            // 리스트 뷰 — ListCtrl 와 짝 (디자인 canonical 페어링).
                            let active_tag = |ui: &mut egui::Ui, th: &Theme| {
                                tag(ui, th, "Active", TagVariant::Success, true);
                            };
                            let items = [
                                ListCtrlItem::new(PRESETS[0].name)
                                    .description(PRESETS[0].desc)
                                    .trailing(&active_tag),
                                ListCtrlItem::new(PRESETS[1].name).description(PRESETS[1].desc),
                                ListCtrlItem::new(PRESETS[2].name).description(PRESETS[2].desc),
                            ];
                            let out = ListCtrl::new().show(ui, th, &items, Some(sel));
                            if let Some(i) = out.clicked {
                                SEL.with(|s| *s.borrow_mut() = i);
                                VIEW.with(|v| *v.borrow_mut() = DrillDownView::Detail);
                            }
                        },
                        |ui, th| {
                            // 디테일 뷰 — 프리셋 프리뷰 (내부 스크롤, back bar 고정).
                            egui::Frame::new()
                                .inner_margin(egui::Margin::same(th.spacing_md.value() as i8))
                                .show(ui, |ui| {
                                    ui.label(
                                        egui::RichText::new(preset.desc)
                                            .size(th.font_size_caption.value())
                                            .color(egui::Color32::from(th.text_muted())),
                                    );
                                    ui.add_space(th.spacing_sm.value());
                                    for (action, key) in preset.bindings {
                                        ui.horizontal(|ui| {
                                            ui.spacing_mut().item_spacing.x =
                                                th.spacing_sm.value();
                                            kbd(ui, th, key);
                                            ui.label(
                                                egui::RichText::new(action)
                                                    .size(th.font_size_body.value())
                                                    .color(egui::Color32::from(
                                                        th.text_secondary(),
                                                    )),
                                            );
                                        });
                                        ui.add_space(th.spacing_xs.value());
                                    }
                                });
                        },
                        Some(&apply),
                    );
                if out.back_clicked {
                    VIEW.with(|v| *v.borrow_mut() = DrillDownView::List);
                }
            });
    });

    meta(
        ui,
        theme,
        &[
            ("backbar", "36 band + hairline"),
            ("swap", "instant (0ms)"),
            ("actions", "detail action slot"),
        ],
        &[
            TokenChip::new(
                "separator",
                "backbar hairline",
                egui::Color32::from(theme.drilldown_backbar_border()),
            ),
            TokenChip::new(
                "text-primary",
                "detail title",
                egui::Color32::from(theme.drilldown_title_fg()),
            ),
            TokenChip::new(
                "accent-primary",
                "Apply action",
                egui::Color32::from(theme.accent_primary()),
            ),
        ],
    );
}
