//! 주제 목록 팝업 — 튜토리얼 진입 표면(CenteredFocused PopupDef). 제목 + 스크롤
//! 가능한 주제 리스트(이름+설명, hover/선택 상태) + "진행" 버튼. 팝업 셸(bg-panel
//! + border-strong + scrim)은 `PopupManager` 가 제공하고, 이 draw_fn 은 내부
//! 콘텐츠만 그린다(headless).
//!
//! 디자인 SoT `gallery/overlays-tutorial.jsx::TopicPopup/Topic` 의 host 대응.
//! "진행" 클릭 → `TutorialRuntime::request_start` 로 시작 큐 + 팝업 close.

use tasty_ui_widgets::tokens::{STRUCT_GAP_2, TUTORIAL_STEP_GAP_X};
use tasty_ui_widgets::{
    Button, ButtonVariant, ControlSize, IconButton, IconButtonVariant, margin_all,
};

use crate::adapters::ui::popup::PopupAction;
use crate::adapters::ui::tutorial::all_topics;
use crate::i18n::t;
use crate::state::AppState;
use crate::theme;

/// 팝업 id. `defs.rs::all_defs()` 및 도구 메뉴 배선에서 참조.
pub const TUTORIAL_TOPICS_POPUP_ID: &str = "tutorial_topics";

/// 기본 크기(360 × 헤더+리스트+푸터). 리스트는 내부 스크롤(max 200).
pub fn tutorial_topics_default_size() -> egui::Vec2 {
    egui::vec2(360.0, 260.0)
}

pub fn draw_tutorial_topics_popup(
    ui: &mut egui::Ui,
    state: &mut AppState,
    _engine: &mut crate::core::CoreState,
) -> PopupAction {
    let th = theme::theme();
    if ui.ctx().input(|i| i.key_pressed(egui::Key::Escape)) {
        return PopupAction::Close;
    }

    let mut action = PopupAction::None;
    ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
    let width = ui.available_width();

    // ── 헤더 (제목 + ✕) ──
    egui::Frame::new()
        .inner_margin(egui::Margin {
            left: th.spacing_lg.value() as i8,
            right: th.spacing_lg.value() as i8,
            top: th.spacing_md.value() as i8,
            bottom: th.spacing_md.value() as i8,
        })
        .show(ui, |ui| {
            ui.set_width(width);
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(t("tutorial.popup_title"))
                        .size(th.font_size_body.value())
                        .strong()
                        .color(th.text_primary().to_egui()),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // 닫기 affordance — banner/갤러리 dismiss_x 와 동일한 Ghost/Sm
                    // IconButton + icons::CLOSE(SVG). raw "✕"(U+2715) 는 UI 폰트에
                    // 글리프가 없어 tofu 위험 + 픽토그래픽 게이트 위반.
                    if IconButton::new()
                        .variant(IconButtonVariant::Ghost)
                        .size(ControlSize::Sm)
                        .show(ui, &th, &|ui, rect, c| {
                            crate::adapters::ui::icons::CLOSE
                                .image(rect.height(), c)
                                .paint_at(ui, rect)
                        })
                        .clicked()
                    {
                        action = PopupAction::Close;
                    }
                });
            });
        });
    hsep(ui, &th, width);

    // ── 주제 리스트 (max-height 200 → 내부 스크롤) ──
    egui::Frame::new()
        .inner_margin(egui::Margin::same(th.spacing_sm.value() as i8))
        .show(ui, |ui| {
            ui.set_width(width);
            egui::ScrollArea::vertical()
                .max_height(th.tutorial_topic_body_max_height().value())
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = th.spacing_xs.value();
                    for (i, topic) in all_topics().iter().enumerate() {
                        let sel = state.tutorial.popup_selected == i;
                        if topic_row(ui, &th, i + 1, t(topic.title_key), t(topic.desc_key), sel) {
                            state.tutorial.popup_selected = i;
                        }
                    }
                });
        });
    hsep(ui, &th, width);

    // ── 푸터 (Esc 힌트 + 진행) ──
    egui::Frame::new()
        .inner_margin(egui::Margin {
            left: th.spacing_lg.value() as i8,
            right: th.spacing_lg.value() as i8,
            top: th.spacing_md.value() as i8,
            bottom: th.spacing_md.value() as i8,
        })
        .show(ui, |ui| {
            ui.set_width(width);
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(t("tutorial.esc_hint"))
                        .monospace()
                        .size(th.font_size_micro.value())
                        .color(th.text_muted().to_egui()),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if Button::new(t("tutorial.btn_start"))
                        .variant(ButtonVariant::Primary)
                        .size(ControlSize::Sm)
                        .show(ui, &th)
                        .clicked()
                    {
                        state.tutorial.request_start(state.tutorial.popup_selected);
                        action = PopupAction::Close;
                    }
                });
            });
        });

    action
}

/// 주제 행 — 인덱스 캡 + 제목/설명. 클릭되면 `true`(선택). 선택 시 surface-active
/// + accent 캡 + accent40% 보더.
fn topic_row(
    ui: &mut egui::Ui,
    th: &tasty_type_appearance::theme::Theme,
    n: usize,
    title: &str,
    desc: &str,
    sel: bool,
) -> bool {
    // 선택된 토픽 테두리 — accent 의 40% alpha. 대응 토큰 없음.
    const SELECTED_BORDER_ALPHA: u8 = 102;
    let border = if sel {
        th.accent_primary()
            .with_alpha(SELECTED_BORDER_ALPHA)
            .to_egui()
    } else {
        egui::Color32::TRANSPARENT
    };
    let fill = if sel {
        th.surface_active().to_egui()
    } else {
        egui::Color32::TRANSPARENT
    };
    let resp = egui::Frame::new()
        .fill(fill)
        .stroke(egui::Stroke::new(th.border_width.value(), border))
        .corner_radius(th.corner_radius.value())
        // 디자인 전사값 10px 유지 — 토큰 산술(4×2.5)로 표현 (그리드 스냅은 디자인 몫).
        .inner_margin(margin_all(th.spacing_xs * 2.5))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = TUTORIAL_STEP_GAP_X;
                let (cap, _) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::hover());
                let (cap_bg, cap_fg) = if sel {
                    (th.accent_primary().to_egui(), th.text_on_accent().to_egui())
                } else {
                    (th.surface_raised().to_egui(), th.text_muted().to_egui())
                };
                ui.painter()
                    .rect_filled(cap, th.corner_radius_sm.value(), cap_bg);
                ui.painter().text(
                    cap.center(),
                    egui::Align2::CENTER_CENTER,
                    n.to_string(),
                    egui::FontId::monospace(th.font_size_micro.value()),
                    cap_fg,
                );
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = STRUCT_GAP_2.value();
                    ui.label(
                        egui::RichText::new(title)
                            .size(th.font_size_body.value())
                            .color(th.text_primary().to_egui()),
                    );
                    ui.label(
                        egui::RichText::new(desc)
                            .size(th.font_size_caption.value())
                            .color(th.text_muted().to_egui()),
                    );
                });
            });
        });
    ui.interact(resp.response.rect, resp.response.id, egui::Sense::click())
        .clicked()
}

fn hsep(ui: &mut egui::Ui, th: &tasty_type_appearance::theme::Theme, width: f32) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(width, th.border_width.value()),
        egui::Sense::hover(),
    );
    ui.painter().hline(
        rect.x_range(),
        rect.center().y,
        egui::Stroke::new(th.border_width.value(), th.separator.to_egui()),
    );
}
