//! Toast 데모 — 디자인(4) `components/feedback/Toast` + `Toast stack` 두 카드.
//!
//! 카드 **한 장**만 보여주는 데모라 스택 함수를 안 부르고 단일 카드 함수
//! (`toast_card::draw_single_card` = `tasty_ui_widgets::draw_toast_single_card`)를 부른다.
//! 그 함수의 치수(galley · 폭 · 높이)와 색(fill · border · accent · alpha 곱)은 본체
//! 스택이 쓰는 `toast_layout_card` · `toast_card_colors` 에서 온다 — 여기서 다시 계산하지
//! 않는다. coalesce / fade / lifetime 등 시간 의존 상태는 본 데모 범위 밖이다.

use tasty_type_appearance::theme::Theme;

use crate::catalog::spec::{StageVariant, TokenChip, meta, note, stage};
use crate::catalog::toast_card::{self, ToastKind};

struct ToastCardProps {
    kind: ToastKind,
    message: &'static str,
}

/// 본체 스택의 카드 1장과 같은 시각 — 치수·색 계산까지 위젯 크레이트의 같은 함수다.
fn draw_toast_card(ui: &mut egui::Ui, theme: &Theme, props: &ToastCardProps, alpha: f32) {
    toast_card::draw_single_card(ui, theme, props.kind, props.message, alpha);
}

/// Toast — 단일 카드 variant (info/success/warning/error).
pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    let cards = [
        ToastCardProps {
            kind: ToastKind::Success,
            message: "Path copied to clipboard",
        },
        ToastCardProps {
            kind: ToastKind::Info,
            message: "This action isn't supported in a mirrored remote explorer yet.",
        },
        ToastCardProps {
            kind: ToastKind::Warning,
            message: "Held by another client (readonly)",
        },
        ToastCardProps {
            kind: ToastKind::Error,
            message: "Force detach — connection dropped",
        },
    ];

    stage(ui, theme, StageVariant::Column, |ui| {
        ui.spacing_mut().item_spacing.y = theme.spacing_sm.value();
        for card in &cards {
            draw_toast_card(ui, theme, card, 1.0);
        }
    });

    meta(
        ui,
        theme,
        &[
            ("rail", "toast-accent-width left"),
            ("radius", "4"),
            ("fill", "surface-raised"),
            ("max-width", "toast-max-width"),
        ],
        &[
            TokenChip::new(
                "accent-success",
                "success rail",
                egui::Color32::from(theme.accent_success()),
            ),
            TokenChip::new(
                "accent-agent",
                "agent rail",
                egui::Color32::from(theme.accent_agent()),
            ),
            TokenChip::new(
                "surface-raised",
                "card fill",
                egui::Color32::from(theme.surface_raised()),
            ),
        ],
    );
}

/// Toast stack — 우측 하단 앵커 스택. 본체 계약(`docs/design/systems/toast.md` 의 스코프 ·
/// 합치기/제한 절)대로 **아래에서 위로 쌓아 가장 새것이 맨 아래**이고, 스코프당 5 장을 넘으면
/// 가장 오래된 것이 즉시 사라진다 — "+N more" 행은 없다.
pub fn draw_stack(ui: &mut egui::Ui, theme: &Theme) {
    // 위에서부터 오래된 순 → 맨 아래가 가장 최근. 본체는 떠 있는 동안 alpha 1 이고
    // 등장/소멸 페이드만 있으므로(나이에 따른 그라데이션 없음) 전부 1.0 으로 그린다.
    let stack = [
        ToastCardProps {
            kind: ToastKind::Warning,
            message: "Held by another client (readonly)",
        },
        ToastCardProps {
            kind: ToastKind::Success,
            message: "Path copied to clipboard",
        },
        ToastCardProps {
            kind: ToastKind::Info,
            message: "This action isn't supported in a mirrored remote explorer yet.",
        },
    ];

    stage(ui, theme, StageVariant::Solo, |ui| {
        // bg-app 영역 위에서 우측 하단 앵커를 흉내내기 위해 우측 정렬.
        egui::Frame::new()
            .fill(egui::Color32::from(theme.bg_app()))
            .inner_margin(egui::Margin::same(theme.spacing_xl.value() as i8))
            .show(ui, |ui| {
                ui.set_width(theme.measure_lg.value());
                ui.with_layout(egui::Layout::top_down(egui::Align::Max), |ui| {
                    ui.spacing_mut().item_spacing.y = theme.spacing_sm.value();
                    for card in &stack {
                        ui.scope(|ui| {
                            ui.set_width(theme.toast_max_width.value());
                            draw_toast_card(ui, theme, card, 1.0);
                        });
                    }
                });
            });
    });

    note(
        ui,
        theme,
        "Anchored bottom-right and stacked bottom-up: the newest card is at the bottom, \
         space-sm gap. At most 5 per scope; a 6th drops the oldest immediately.",
    );

    meta(
        ui,
        theme,
        &[
            ("anchor", "bottom-right"),
            ("order", "newest bottom"),
            ("gap", "space-sm 8"),
            ("cap", "5 per scope → oldest dropped"),
        ],
        &[
            TokenChip::new(
                "space-sm",
                "card gap",
                egui::Color32::from(theme.accent_primary()),
            ),
            TokenChip::new(
                "surface-raised",
                "card fill",
                egui::Color32::from(theme.surface_raised()),
            ),
        ],
    );
}
