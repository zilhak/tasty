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

/// Toast stack — 우측 하단 앵커 스택(newest top) + "+N more" overflow 행.
pub fn draw_stack(ui: &mut egui::Ui, theme: &Theme) {
    // newest top: 위에서부터 가장 최근. fade 그라데이션으로 오래된 카드일수록 옅게.
    let stack = [
        (
            ToastCardProps {
                kind: ToastKind::Info,
                message: "This action isn't supported in a mirrored remote explorer yet.",
            },
            1.0,
        ),
        (
            ToastCardProps {
                kind: ToastKind::Success,
                message: "Path copied to clipboard",
            },
            0.85,
        ),
        (
            ToastCardProps {
                kind: ToastKind::Warning,
                message: "Held by another client (readonly)",
            },
            0.6,
        ),
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
                    for (card, alpha) in &stack {
                        ui.scope(|ui| {
                            ui.set_width(theme.toast_max_width.value());
                            draw_toast_card(ui, theme, card, *alpha);
                        });
                    }
                    // "+N more" overflow 행 (height 22 ≈ control-height-tree).
                    let (r, _) = ui.allocate_exact_size(
                        egui::vec2(
                            theme.toast_max_width.value(),
                            theme.item_height_tree.value(),
                        ),
                        egui::Sense::hover(),
                    );
                    ui.painter().text(
                        r.center(),
                        egui::Align2::CENTER_CENTER,
                        "+2 more",
                        egui::FontId::proportional(theme.font_size_caption.value()),
                        egui::Color32::from(theme.text_muted()),
                    );
                });
            });
    });

    note(
        ui,
        theme,
        "Anchored bottom-right, newest on top, space-sm gap. Beyond the visible cap the stack \
         collapses to a +N more row.",
    );

    meta(
        ui,
        theme,
        &[
            ("anchor", "bottom-right"),
            ("order", "newest top"),
            ("gap", "space-sm 8"),
            ("cap", "N → +N more"),
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
            TokenChip::new(
                "text-muted",
                "+N more",
                egui::Color32::from(theme.text_muted()),
            ),
        ],
    );
}
