//! egui-mesh popup 콘텐츠 렌더 — master-detail 자가 렌더 (B4).
//!
//! 좌측 = 가용 클립보드 타입 목록(Button: 선택=primary 채움, 유휴=secondary 외곽선),
//! 우측 = 선택 타입 상세(mono 미리보기). 빈/읽기실패/이미열림 상태는 중앙 한 줄.
//! chrome(scrim/border/outside-click/Esc)은 host 소유 — plugin 은 content 영역만 그린다.
//! 색·폰트·간격은 전부 host 가 보낸 `Theme` 토큰에서 가져온다(from_rgb/raw px 금지).

use tasty_plugin_sdk::Translator;
use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{Button, ButtonVariant, ControlSize};

use crate::ViewerState;
use crate::clipboard::{ClipboardType, ContentRepr};

/// 좌측 타입 목록 비율 (기존 UiNode `splitter(Horizontal, 0.3, ..)` 전사).
const LEFT_RATIO: f32 = 0.3;

/// 주 인스턴스 popup 본문. read_error / empty / master-detail 3분기(기존 `main_tree` 동형).
pub fn draw(ctx: &egui::Context, theme: &Theme, state: &mut ViewerState, tr: &Translator) {
    panel(ctx, theme, |ui| {
        // 읽기 실패 → accent_danger 중앙 한 줄.
        if state.read_error.is_some() {
            centered(
                ui,
                theme,
                tr.t("clipboard_viewer.popup.read_failed"),
                theme.accent_danger().to_egui(),
            );
            return;
        }
        // 가용 타입 0개 → text_muted 빈 상태.
        if state.available.is_empty() {
            centered(
                ui,
                theme,
                tr.t("clipboard_viewer.popup.empty"),
                theme.text_muted().to_egui(),
            );
            return;
        }
        master_detail(ui, theme, state, tr);
    });
}

/// 단일 인스턴스 가드 placeholder — "이미 열림" 중앙 한 줄(기존 `already_open_tree` 동형).
pub fn draw_already_open(ctx: &egui::Context, theme: &Theme, tr: &Translator) {
    panel(ctx, theme, |ui| {
        centered(
            ui,
            theme,
            tr.t("clipboard_viewer.popup.already_open"),
            theme.text_muted().to_egui(),
        );
    });
}

/// popup content 영역을 채우는 CentralPanel. host 셸이 그린 `bg_panel` 과 이음매 없게
/// 동일 토큰으로 채운다(패딩은 컬럼별로 `spacing_sm` 적용 — 이중 여백 방지 위해 margin 0).
fn panel(ctx: &egui::Context, theme: &Theme, add: impl FnOnce(&mut egui::Ui)) {
    let frame = egui::Frame::new().fill(theme.bg_panel().to_egui());
    egui::CentralPanel::default()
        .frame(frame)
        .show(ctx, |ui| add(ui));
}

/// 정상 상태 — 좌 타입 버튼 목록(선택=primary) | 1px separator | 우 mono 미리보기.
fn master_detail(ui: &mut egui::Ui, theme: &Theme, state: &mut ViewerState, tr: &Translator) {
    let full = ui.available_rect_before_wrap();
    let pad = theme.spacing_sm.value();
    let split_x = (full.left() + full.width() * LEFT_RATIO).round();

    // 좌우 분할선 — 기존 host splitter 의 rest 색(separator), 1px.
    ui.painter().vline(
        split_x,
        full.y_range(),
        egui::Stroke::new(theme.border_width.value(), theme.separator.to_egui()),
    );

    // ── 좌측 타입 목록 ──
    let left_rect = egui::Rect::from_min_max(
        egui::pos2(full.left() + pad, full.top() + pad),
        egui::pos2(split_x - pad, full.bottom() - pad),
    );
    let mut left_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(left_rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .id_salt("clip_types")
        .show(&mut left_ui, |ui| {
            // `state.available` 을 먼저 owned 키 목록으로 뽑아 loop 중 `state.selected`
            // mutable 접근과의 차용 충돌을 피한다(`ClipboardType` 은 Copy).
            let types: Vec<ClipboardType> = state.available.iter().map(|(t, _)| *t).collect();
            for ty in types {
                let selected = state.selected == Some(ty);
                let variant = if selected {
                    ButtonVariant::Primary
                } else {
                    ButtonVariant::Secondary
                };
                let label = tr.t(ty.label_i18n_key());
                let resp = Button::new(label)
                    .variant(variant)
                    .size(ControlSize::Md)
                    .block(true)
                    .show(ui, theme);
                if resp.clicked() {
                    state.selected = Some(ty);
                }
                ui.add_space(theme.spacing_xs.value());
            }
        });

    // ── 우측 미리보기 ──
    let preview = state
        .selected
        .and_then(|ty| state.available.iter().find(|(t, _)| *t == ty))
        .map(|(_, ContentRepr::Text(s))| s.clone());
    let right_rect = egui::Rect::from_min_max(
        egui::pos2(split_x + pad, full.top() + pad),
        egui::pos2(full.right() - pad, full.bottom() - pad),
    );
    let mut right_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(right_rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .id_salt("clip_preview")
        .show(&mut right_ui, |ui| {
            ui.style_mut().interaction.selectable_labels = true;
            if let Some(text) = preview {
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(text)
                            .monospace()
                            .size(theme.font_size_body.value())
                            .color(theme.text_primary().to_egui()),
                    )
                    .wrap(),
                );
            }
        });
}

/// 빈/읽기실패/이미열림 — content 영역 양축 중앙에 메시지 한 줄.
fn centered(ui: &mut egui::Ui, theme: &Theme, msg: &str, color: egui::Color32) {
    let h = ui.available_height().max(1.0);
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), h),
        egui::Layout::centered_and_justified(egui::Direction::TopDown),
        |ui| {
            ui.label(
                egui::RichText::new(msg)
                    .size(theme.font_size_body.value())
                    .color(color),
            );
        },
    );
}
