//! Toast 데모 — 디자인(4) `components/feedback/Toast` + `Toast stack` 두 카드.
//!
//! 본체 `src/adapters/ui/toast.rs::ToastManager::draw` 의 *카드 시각* 만 재현
//! (coalesce / fade / lifetime 등 시간 의존 상태는 본 데모 범위 밖). 색·치수는
//! 모두 `Theme` 토큰. `ToastKind` 는 본체 정본(`crates/tasty-model`)과 **kind-for-kind
//! 동일**한 분류를 `toast_card` 모듈에 로컬 정의한다 — 정본 크레이트가 터미널 모델까지
//! 끌고 오기 때문이고, 종류를 갤러리가 임의로 늘리지는 않는다.

use tasty_type_appearance::theme::Theme;

use crate::catalog::spec::{StageVariant, TokenChip, meta, note, stage};
use crate::catalog::toast_card::{self, ACCENT_BAR_WIDTH, PADDING_X, PADDING_Y, ToastKind};

struct ToastCardProps {
    kind: ToastKind,
    message: &'static str,
}

/// 본체 `ToastManager::draw` 의 카드 1장 그리기와 동등한 시각.
fn draw_toast_card(ui: &mut egui::Ui, theme: &Theme, props: &ToastCardProps, alpha: f32) {
    let bg = egui::Color32::from(theme.surface_raised()).gamma_multiply(alpha);
    let border = egui::Color32::from(theme.border_default()).gamma_multiply(alpha);
    let accent = toast_card::accent_color(props.kind, theme).gamma_multiply(alpha);
    let text_color = egui::Color32::from(theme.text_primary()).gamma_multiply(alpha);

    let max_width = theme.toast_max_width.value();
    let font = egui::FontId::proportional(theme.font_size_body.value());

    let galley = ui.ctx().fonts(|f| {
        f.layout(
            props.message.to_string(),
            font.clone(),
            text_color,
            max_width - PADDING_X * 2.0 - ACCENT_BAR_WIDTH,
        )
    });

    let toast_w = (galley.size().x + PADDING_X * 2.0 + ACCENT_BAR_WIDTH).min(max_width);
    let toast_h = galley.size().y + PADDING_Y * 2.0;

    let (rect, _) = ui.allocate_exact_size(egui::vec2(toast_w, toast_h), egui::Sense::hover());
    let painter = ui.painter_at(rect);

    toast_card::draw_card(
        &painter,
        theme,
        rect,
        toast_card::CardColors {
            bg,
            border,
            accent,
            text: text_color,
        },
        galley,
    );
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
