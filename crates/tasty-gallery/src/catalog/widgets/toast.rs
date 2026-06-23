//! Toast 데모 (시각 패턴만).
//!
//! 본체 `src/adapters/ui/toast.rs::ToastManager::draw` 의 *카드 시각* 만 재현.
//! coalesce / fade / lifetime 등 *시간 의존 상태 관리* 는 본 POC 범위 밖
//! (해당 부분은 Tier 3 로 분리됨 — see `.claude-workspace/conductor/tier-3-props-extraction-pattern.md`).
//!
//! 본체 의존: 없음. `Theme.{surface0, surface1, blue, green, yellow, red, text, ...}` 색만 사용.
//! `ToastKind` 는 본체 `crates/tasty-model/src/toast_kind.rs` 와 같은 분류를 로컬에 정의
//! (gallery 가 `tasty-model` 의 전부를 끌어오는 것을 피하기 위해).

use tasty_type_appearance::theme::Theme;

use crate::catalog::toast_card::{self, ACCENT_BAR_WIDTH, PADDING_X, PADDING_Y, ToastKind};

struct ToastCardProps {
    kind: ToastKind,
    message: &'static str,
}

fn kind_label(kind: ToastKind) -> &'static str {
    match kind {
        ToastKind::Info => "Info",
        ToastKind::Success => "Success",
        ToastKind::Warning => "Warning",
        ToastKind::Error => "Error",
    }
}

/// 본체 `ToastManager::draw` 의 카드 1장 그리기와 동등한 시각.
fn draw_toast_card(ui: &mut egui::Ui, theme: &Theme, props: &ToastCardProps) {
    let bg = egui::Color32::from(theme.surface0);
    let border = egui::Color32::from(theme.surface1);
    let accent = toast_card::accent_color(props.kind, theme);
    let text_color = egui::Color32::from(theme.text);

    let max_width = 320.0;
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

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    ui.label(
        egui::RichText::new("ToastManager::draw — card visual (single frame, no lifecycle)")
            .small()
            .color(egui::Color32::from(theme.subtext0)),
    );
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new(
            "accent color: theme.{blue|green|yellow|red} | bg: theme.surface0 | border: theme.surface1",
        )
        .small()
        .color(egui::Color32::from(theme.subtext0)),
    );
    ui.add_space(12.0);

    let cards = [
        ToastCardProps {
            kind: ToastKind::Info,
            message: "Reloaded settings.json",
        },
        ToastCardProps {
            kind: ToastKind::Success,
            message: "Workspace saved.",
        },
        ToastCardProps {
            kind: ToastKind::Warning,
            message: "Low disk space — clean up downloads.",
        },
        ToastCardProps {
            kind: ToastKind::Error,
            message: "Plugin crashed: tasty-plugin-foo. See logs.",
        },
    ];

    for card in &cards {
        ui.label(
            egui::RichText::new(kind_label(card.kind))
                .small()
                .color(egui::Color32::from(theme.subtext0)),
        );
        ui.add_space(2.0);
        draw_toast_card(ui, theme, card);
        ui.add_space(10.0);
    }
}
