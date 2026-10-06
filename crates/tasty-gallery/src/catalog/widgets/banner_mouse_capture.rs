//! 마우스 캡처 배너의 상태 예제. 시안 `overlays-banners.jsx` 의 "Anatomy & states".
//! 첫 카드는 직전 프레임의 카드 영역에 포인터가 있으면 ⋯/× 를 드러낸다.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::banner_shell;

use super::banner::{MoreTriggerState, caption_label, mouse_capture_banner_body};
use crate::catalog::spec::{self, StageVariant, TokenChip};

// 시안 무대의 전시 치수. 대응 토큰이 없다.
/// 무대 안쪽 여백 — `padding: 20`.
const STAGE_PAD: LogicalPx = LogicalPx(20.0);
/// 예제 칸 최대 폭 — `maxWidth: 480`.
const CARD_MAX_W: LogicalPx = LogicalPx(480.0);

/// bg-app 무대 — 가운데 정렬 세로 묶음.
fn app_stage(ui: &mut egui::Ui, theme: &Theme, add: impl FnOnce(&mut egui::Ui)) {
    spec::stage(ui, theme, StageVariant::Tight, |ui| {
        egui::Frame::new()
            .fill(theme.bg_app().to_egui())
            .corner_radius(theme.corner_radius.value())
            .inner_margin(egui::Margin::same(STAGE_PAD.value() as i8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.vertical_centered(add);
            });
    });
}

/// 라벨 + 480 폭 카드 한 칸.
fn card_slot(ui: &mut egui::Ui, theme: &Theme, label: &str, add: impl FnOnce(&mut egui::Ui)) {
    let w = CARD_MAX_W.value().min(ui.available_width());
    ui.allocate_ui_with_layout(
        egui::vec2(w, 0.0),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            ui.set_width(w);
            ui.spacing_mut().item_spacing.y = theme.spacing_sm.value();
            caption_label(ui, theme, label);
            add(ui);
        },
    );
}

pub fn draw_anatomy(ui: &mut egui::Ui, theme: &Theme) {
    app_stage(ui, theme, |ui| {
        ui.spacing_mut().item_spacing.y = theme.spacing_lg.value();
        card_slot(ui, theme, "default · hover the card to reveal ×", |ui| {
            let id = ui.id().with("g_banner_anatomy_card");
            let prev: Option<egui::Rect> = ui.data(|d| d.get_temp(id));
            let hovered = prev.is_some_and(|r| ui.rect_contains_pointer(r));
            let state = if hovered {
                MoreTriggerState::Hovered
            } else {
                MoreTriggerState::Hidden
            };
            let top = ui.cursor().min;
            banner_shell(ui, theme, 1.0, |ui| {
                mouse_capture_banner_body(ui, theme, state, None);
            });
            let rect = egui::Rect::from_min_max(top, ui.min_rect().max);
            ui.data_mut(|d| d.insert_temp(id, rect));
        });
        card_slot(ui, theme, "one-line body (short locale)", |ui| {
            banner_shell(ui, theme, 1.0, |ui| {
                mouse_capture_banner_body(
                    ui,
                    theme,
                    MoreTriggerState::Hidden,
                    Some("Mouse captured — hold Shift to select."),
                );
            });
        });
        card_slot(
            ui,
            theme,
            "recessed — dimmed behind a higher-scope banner",
            |ui| {
                // 시안은 카드 전체(셸 + 내용)에 opacity 를 건다. banner_shell 의 opacity 는 셸만 흐린다.
                ui.scope(|ui| {
                    ui.multiply_opacity(theme.opacity_recessed());
                    banner_shell(ui, theme, 1.0, |ui| {
                        mouse_capture_banner_body(ui, theme, MoreTriggerState::Hidden, None);
                    });
                });
            },
        );
    });

    spec::meta(
        ui,
        theme,
        &[
            (
                "title",
                "\"Mouse input captured\" · 13/600 --tasty-banner-fg",
            ),
            ("body", "caption · --tasty-text-muted · 1–3 lines"),
            ("glyph", "mouse · --tasty-banner-icon-fg"),
            ("affordance", "⋯ + × on hover (no TTL / countdown)"),
            ("fires", "once per tracking session · user click only"),
            ("dismiss", "suppresses for the session"),
        ],
        &[
            TokenChip::new("banner-bg", "card fill", theme.banner_bg().to_egui()),
            TokenChip::new(
                "banner-icon-fg",
                "mouse glyph",
                theme.banner_icon_fg().to_egui(),
            ),
            TokenChip::without_color("banner-recessed-opacity", "dimmed variant"),
        ],
    );
    spec::note(
        ui,
        theme,
        "The bypass keys (Shift+drag / Shift+Right-click) are the message — keep them in the \
         body. No inline action buttons: the body stays text-only, and the two per-app opt-outs \
         live behind the ⋯ trigger next to the × (see Banner more menu below).",
    );
}
