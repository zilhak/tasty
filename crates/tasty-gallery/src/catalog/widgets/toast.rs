//! 본체와 같은 단일 카드 그리기 함수로 토스트 종류와 쌓이는 순서를 비교한다.
//! 치수·색 계산을 공유하며 수명·페이드·중복 합치기는 실행하지 않는다.

use tasty_type_appearance::theme::Theme;

use crate::catalog::spec::{StageVariant, TokenChip, dont, meta, note, stage};
use crate::catalog::toast_card::{self, ToastKind};

struct ToastCardProps {
    kind: ToastKind,
    message: &'static str,
}

fn draw_toast_card(ui: &mut egui::Ui, theme: &Theme, props: &ToastCardProps, alpha: f32) {
    toast_card::draw_single_card(ui, theme, props.kind, props.message, alpha);
}

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

/// 가장 새 토스트가 아래에 오도록 배치한다. 본체는 스코프당 다섯 개를 넘으면 가장 오래된 것을 지운다.
/// 나이에 따라 색이 흐려지지 않으므로 모든 예제의 불투명도는 1로 둔다.
pub fn draw_stack(ui: &mut egui::Ui, theme: &Theme) {
    // 위가 가장 오래된 것, 아래가 가장 새 것이다.
    let stack = [
        ToastCardProps {
            kind: ToastKind::Info,
            message: "Two notices while importing the bundle",
        },
        ToastCardProps {
            kind: ToastKind::Warning,
            message: "Held by another client (readonly)",
        },
        // 시안의 agent 변형은 본체 ToastKind에 없어 Info 강조색으로 대신한다.
        ToastCardProps {
            kind: ToastKind::Info,
            message: "Agent opened 3 surfaces in background",
        },
        ToastCardProps {
            kind: ToastKind::Success,
            message: "Path copied to clipboard",
        },
        ToastCardProps {
            kind: ToastKind::Error,
            message: "Force detach — connection dropped",
        },
    ];

    stage(ui, theme, StageVariant::Solo, |ui| {
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
         space-sm gap. At most 5 per scope; a 6th drops the oldest (topmost) immediately. \
         Cards past the scope's top edge are not drawn, and every resting card is fully \
         opaque — alpha is only for enter and exit.",
    );

    note(
        ui,
        theme,
        "The host has no agent ToastKind and no hint slot yet, so the agent card uses the \
         Info accent and the success card omits its hint.",
    );

    meta(
        ui,
        theme,
        &[
            ("anchor", "bottom-right"),
            ("order", "newest bottom"),
            ("gap", "space-sm 8"),
            ("cap", "5 per scope → oldest dropped"),
            ("width", "~320–380px"),
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

    dont(
        ui,
        theme,
        "Don't let the stack grow unbounded, and don't fold the tail into a \"+N more\" row \
         either. Hold the cap by dropping the oldest card.",
    );
}
