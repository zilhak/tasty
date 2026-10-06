//! 본체 마크다운 렌더러가 그리는 콜아웃 다섯 종(아이콘·라벨·채움). 시안 Callouts 표본에는
//! 없는 본체 전용 모양이라 문서 카드와 따로 둔다.

use tasty_type_appearance::color::HexColor;
use tasty_type_appearance::theme::Theme;

use super::{DOC_W, rich};
use crate::catalog::icons;
use crate::catalog::spec::{self, StageVariant, TokenChip};

/// 콜아웃 한 종의 아이콘·강조색·라벨·본문.
type CalloutKind = (
    icons::MockGlyph,
    fn(&Theme) -> HexColor,
    &'static str,
    &'static str,
);

const KINDS: [CalloutKind; 5] = [
    (
        icons::ALERT_CIRCLE,
        Theme::accent_primary,
        "Note",
        "Highlights information users should take into account, even when skimming.",
    ),
    (
        icons::STAR_FILL,
        Theme::accent_success,
        "Tip",
        "Optional information to help a user be more successful.",
    ),
    (
        icons::BELL,
        Theme::accent_agent,
        "Important",
        "Crucial information necessary for users to succeed.",
    ),
    (
        icons::ALERT_TRIANGLE,
        Theme::accent_warning,
        "Warning",
        "Critical content demanding immediate user attention due to possible risks.",
    ),
    (
        icons::CLOSE,
        Theme::accent_danger,
        "Caution",
        "Negative potential consequences of an action.",
    ),
];

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Column, |ui| {
        ui.set_max_width(DOC_W.value());
        egui::Frame::new()
            .fill(theme.bg_app().to_egui())
            .stroke(egui::Stroke::new(
                theme.border_width.value(),
                theme.border_default().to_egui(),
            ))
            .corner_radius(theme.corner_radius.value())
            .inner_margin(egui::Margin::symmetric(
                theme.spacing_lg.value() as i8,
                theme.spacing_md.value() as i8,
            ))
            .show(ui, |ui| {
                ui.set_width(DOC_W.value());
                for (icon, accent, label, body) in KINDS {
                    callout_box(ui, theme, icon, accent(theme), label, body);
                    ui.add_space(theme.spacing_xs.value());
                }
            });
    });
    spec::meta(
        ui,
        theme,
        &[
            (
                "kinds",
                "note · tip · important · warning · caution (+ Obsidian aliases)",
            ),
            (
                "box",
                "type colour 12% fill · md-quote-bar-width left bar · radius · icon + label",
            ),
            (
                "status",
                "본체 전용, 요청 explorer-header-font-and-callout-fill 회신 대기",
            ),
        ],
        &[
            TokenChip::new("accent-primary", "note", theme.accent_primary().to_egui()),
            TokenChip::new("accent-success", "tip", theme.accent_success().to_egui()),
            TokenChip::new("accent-agent", "important", theme.accent_agent().to_egui()),
            TokenChip::new(
                "accent-warning",
                "warning",
                theme.accent_warning().to_egui(),
            ),
            TokenChip::new("accent-danger", "caution", theme.accent_danger().to_egui()),
            TokenChip::without_color("md-quote-bar-width", "left bar"),
        ],
    );
}

/// 강조색 12% 채움(`render.rs::alert_css`의 `BG_ALPHA = 31`) + 왼쪽 md-quote-bar-width 막대 +
/// 아이콘과 굵은 라벨 + 본문.
fn callout_box(
    ui: &mut egui::Ui,
    theme: &Theme,
    icon: icons::MockGlyph,
    color: HexColor,
    label: &str,
    body: &str,
) {
    // 콜아웃 채움 — 강조색 저알파. 대응 토큰이 없어 본체 렌더러와 같은 값을 둔다.
    const CALLOUT_BG_ALPHA: u8 = 31;
    let resp = egui::Frame::new()
        .fill(color.with_alpha(CALLOUT_BG_ALPHA).to_egui())
        .corner_radius(theme.corner_radius.value())
        .inner_margin(egui::Margin::symmetric(
            theme.spacing_md.value() as i8,
            theme.spacing_sm.value() as i8,
        ))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let sz = theme.font_size_body.value();
                let (rect, _) = ui.allocate_exact_size(egui::vec2(sz, sz), egui::Sense::hover());
                icon.image(sz, color.to_egui()).paint_at(ui, rect);
                ui.label(
                    rich(theme, label, theme.font_size_body.value(), color.to_egui()).strong(),
                );
            });
            ui.add_space(theme.spacing_xs.value());
            ui.label(rich(
                theme,
                body,
                theme.font_size_body.value(),
                theme.text_secondary().to_egui(),
            ));
        });
    let r = resp.response.rect;
    let bar = egui::Rect::from_min_size(
        r.min,
        egui::vec2(theme.md_quote_bar_width().value(), r.height()),
    );
    ui.painter().rect_filled(bar, 0.0, color.to_egui());
}
