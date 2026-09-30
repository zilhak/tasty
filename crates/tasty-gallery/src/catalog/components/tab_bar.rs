//! 페인 탭바의 활성 표시와 응답 대기·완료 알림 색을 비교하는 정적 예제.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;

use crate::catalog::icons::{CHEVRON_LEFT, CHEVRON_RIGHT, MockGlyph, PLUS, SEARCH, SPLIT};
use crate::catalog::spec::{self, StageVariant, TokenChip};

const TABS: &[(&str, bool)] = &[("README.md", false), ("build.rs", true), ("run.rs", false)];

/// 응답 대기 > 완료 > 활성 > 평상시 순서의 제목 색을 보여주는 예제.
const ATTENTION_TABS: &[(&str, bool, Option<Kind>)] = &[
    ("waiting.rs", false, Some(Kind::NeedsInput)),
    ("done.rs", true, Some(Kind::Completion)),
    ("idle.rs", false, None),
];

#[derive(Clone, Copy)]
enum Kind {
    NeedsInput,
    Completion,
}

fn strip(ui: &mut egui::Ui, theme: &Theme) {
    let bar_h = theme.tab_bar_height.value(); // 24 — 본체와 같은 토큰(zoom 비적용)
    let tab_w = theme.tab_width.value(); // 150
    let w = ui.available_width().min(theme.measure_xl.value());
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, bar_h), egui::Sense::hover());
    let p = ui.painter_at(rect);

    p.rect_filled(rect, 0.0, egui::Color32::from(theme.bg_sidebar()));
    p.hline(
        rect.x_range(),
        rect.max.y - theme.border_width.value() * 0.5,
        egui::Stroke::new(
            theme.border_width.value(),
            egui::Color32::from(theme.border_default()),
        ),
    );

    let font = egui::FontId::proportional(theme.tab_bar_label_font_size.value());
    let mut x = rect.min.x;
    for (i, (name, active)) in TABS.iter().enumerate() {
        let tab = egui::Rect::from_min_size(egui::pos2(x, rect.min.y), egui::vec2(tab_w, bar_h));
        if *active {
            p.rect_filled(tab, 0.0, egui::Color32::from(theme.bg_panel()));
            let bar = egui::Rect::from_min_size(
                tab.min,
                egui::vec2(tab_w, theme.tab_indicator_width.value()),
            );
            p.rect_filled(bar, 0.0, egui::Color32::from(theme.accent_primary()));
        }
        if i > 0 {
            p.vline(
                x,
                rect.y_range(),
                egui::Stroke::new(
                    theme.border_width.value(),
                    // 이미 premultiply된 구분선 색에 알파를 다시 곱하지 않는다.
                    theme.tab_separator().to_egui_premultiplied(),
                ),
            );
        }
        p.text(
            egui::pos2(tab.min.x + theme.spacing_sm.value(), tab.center().y),
            egui::Align2::LEFT_CENTER,
            name,
            font.clone(),
            egui::Color32::from(if *active {
                theme.text_primary()
            } else {
                theme.text_muted()
            }),
        );
        x += tab_w;
    }

    let icon = theme.icon_glyph_size_md.value();
    let plus_rect = egui::Rect::from_min_size(egui::pos2(x, rect.min.y), egui::vec2(bar_h, bar_h));
    paint_icon(
        ui,
        PLUS,
        plus_rect,
        icon,
        egui::Color32::from(theme.text_secondary()),
    );

    let search_rect = egui::Rect::from_min_size(
        egui::pos2(rect.max.x - bar_h, rect.min.y),
        egui::vec2(bar_h, bar_h),
    );
    let split_rect = search_rect.translate(egui::vec2(-bar_h, 0.0));
    paint_icon(
        ui,
        SPLIT,
        split_rect,
        icon,
        egui::Color32::from(theme.text_secondary()),
    );
    paint_icon(
        ui,
        SEARCH,
        search_rect,
        icon,
        egui::Color32::from(theme.text_secondary()),
    );
}

fn paint_icon(
    ui: &mut egui::Ui,
    glyph: crate::catalog::icons::MockGlyph,
    area: egui::Rect,
    size: f32,
    color: egui::Color32,
) {
    let icon_rect = egui::Rect::from_center_size(area.center(), egui::vec2(size, size));
    glyph.image(size, color).paint_at(ui, icon_rect);
}

/// 탭 제목 색 위계 데모 — 본체 `tab_bar/tab.rs` 의 `text_color` 분기(NeedsInput → \
/// Completion → active → 평상시)를 3탭으로 재현.
fn attention_strip(ui: &mut egui::Ui, theme: &Theme) {
    let bar_h = theme.tab_bar_height.value();
    let tab_w = theme.tab_width.value();
    let w = ui.available_width().min(theme.measure_xl.value());
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, bar_h), egui::Sense::hover());
    let p = ui.painter_at(rect);

    p.rect_filled(rect, 0.0, egui::Color32::from(theme.bg_sidebar()));
    p.hline(
        rect.x_range(),
        rect.max.y - theme.border_width.value() * 0.5,
        egui::Stroke::new(
            theme.border_width.value(),
            egui::Color32::from(theme.border_default()),
        ),
    );

    let font = egui::FontId::proportional(theme.tab_bar_label_font_size.value());
    let mut x = rect.min.x;
    for (i, (name, active, kind)) in ATTENTION_TABS.iter().enumerate() {
        let tab = egui::Rect::from_min_size(egui::pos2(x, rect.min.y), egui::vec2(tab_w, bar_h));
        if *active {
            p.rect_filled(tab, 0.0, egui::Color32::from(theme.bg_panel()));
            let bar = egui::Rect::from_min_size(
                tab.min,
                egui::vec2(tab_w, theme.tab_indicator_width.value()),
            );
            p.rect_filled(bar, 0.0, egui::Color32::from(theme.accent_primary()));
        }
        if i > 0 {
            p.vline(
                x,
                rect.y_range(),
                egui::Stroke::new(
                    theme.border_width.value(),
                    // 이미 premultiply된 구분선 색에 알파를 다시 곱하지 않는다.
                    theme.tab_separator().to_egui_premultiplied(),
                ),
            );
        }
        let text_color = match kind {
            Some(Kind::NeedsInput) => theme.accent_warning(),
            Some(Kind::Completion) => theme.accent_primary(),
            None if *active => theme.text_primary(),
            None => theme.text_muted(),
        };
        p.text(
            egui::pos2(tab.min.x + theme.spacing_sm.value(), tab.center().y),
            egui::Align2::LEFT_CENTER,
            *name,
            font.clone(),
            egui::Color32::from(text_color),
        );
        x += tab_w;
    }
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Solo, |ui| {
        strip(ui, theme);
    });
    spec::stage(ui, theme, StageVariant::Solo, |ui| {
        attention_strip(ui, theme);
    });

    spec::meta(
        ui,
        theme,
        &[
            ("tab-height", "24 (control-height-tab)"),
            ("tab-width", "150"),
            ("strip", "bg-sidebar + bottom border"),
            ("active", "bg-panel + accent top bar"),
            ("controls", "+ · Split · Search"),
            (
                "title color order",
                "needs-input(yellow) → completion(blue) → active(text-primary) → text-muted",
            ),
        ],
        &[
            TokenChip::new("bg-sidebar", "strip fill", theme.bg_sidebar().into()),
            TokenChip::new("bg-panel", "active tab", theme.bg_panel().into()),
            TokenChip::new(
                "accent-primary",
                "active top bar · completion title",
                theme.accent_primary().into(),
            ),
            TokenChip::new(
                "accent-warning",
                "needs-input title",
                theme.accent_warning().into(),
            ),
            TokenChip::new(
                "tab-separator",
                "tab divider",
                theme.tab_separator().to_egui_premultiplied(),
            ),
        ],
    );

    spec::note(
        ui,
        theme,
        "활성 탭은 bg-panel 배경과 상단 accent 선으로 표시한다. 제목 색은 응답 대기, 완료, 활성 순서로 고른다. 실제 사용자 포커스를 받으면 알림이 해제되지만, 이 예제는 활성 상태와 알림이 함께 있을 때의 우선순위도 보여준다.",
    );
}

/// 스크롤 화살표 예제 스트립의 폭. 디자인 Disabled ink Spec의 C4 행이 `--tasty-size-288`을 쓴다.
const SCROLL_STRIP_W: LogicalPx = LogicalPx(288.0);

/// 디자인 C4 행의 탭 스트립 스크롤 화살표 칸: surface-raised 칸에 chevron을 그린다.
/// 시안은 스트립의 `overflow: hidden` + `radius-sm`으로 칸의 바깥 모서리를 자르므로
/// `corners`에는 스트립 끝에 닿는 두 모서리만 반경을 준다.
fn arrow_cell(
    ui: &egui::Ui,
    theme: &Theme,
    cell: egui::Rect,
    corners: egui::CornerRadius,
    glyph: MockGlyph,
    ink: egui::Color32,
) {
    ui.painter()
        .rect_filled(cell, corners, egui::Color32::from(theme.surface_raised()));
    let size = theme.icon_glyph_size_xs.value();
    glyph.image(size, ink).paint_at(
        ui,
        egui::Rect::from_center_size(cell.center(), egui::vec2(size, size)),
    );
}

/// 왼쪽 끝까지 스크롤한 스트립: `<`는 disabled, `>`는 enabled 잉크.
fn scroll_strip(ui: &mut egui::Ui, theme: &Theme) {
    let h = theme.item_height_tab.value();
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(SCROLL_STRIP_W.value(), h), egui::Sense::hover());
    ui.painter().rect_filled(
        rect,
        theme.corner_radius_sm.value(),
        egui::Color32::from(theme.bg_sidebar()),
    );
    let left = egui::Rect::from_min_size(rect.min, egui::vec2(h, h));
    let right =
        egui::Rect::from_min_size(egui::pos2(rect.right() - h, rect.top()), egui::vec2(h, h));
    let r = theme.corner_radius_sm.value() as u8;
    arrow_cell(
        ui,
        theme,
        left,
        egui::CornerRadius {
            nw: r,
            sw: r,
            ..egui::CornerRadius::ZERO
        },
        CHEVRON_LEFT,
        egui::Color32::from(theme.tab_scroll_arrow_fg_disabled()),
    );
    arrow_cell(
        ui,
        theme,
        right,
        egui::CornerRadius {
            ne: r,
            se: r,
            ..egui::CornerRadius::ZERO
        },
        CHEVRON_RIGHT,
        egui::Color32::from(theme.tab_scroll_arrow_fg()),
    );
    ui.painter_at(rect).text(
        egui::pos2(left.right() + theme.spacing_sm.value(), rect.center().y),
        egui::Align2::LEFT_CENTER,
        "server · dev · vim · logs",
        egui::FontId::proportional(theme.font_size_caption.value()),
        egui::Color32::from(theme.text_secondary()),
    );
}

/// 순서 규칙 사다리: placeholder < disabled < muted < secondary < primary.
fn ink_ladder(ui: &mut egui::Ui, theme: &Theme) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = theme.spacing_xs.value();
        for (name, ink) in [
            ("placeholder", theme.text_placeholder()),
            ("disabled", theme.text_disabled()),
            ("muted", theme.text_muted()),
            ("secondary", theme.text_secondary()),
            ("primary", theme.text_primary()),
        ] {
            egui::Frame::new()
                .fill(egui::Color32::from(theme.bg_panel()))
                .corner_radius(theme.corner_radius_sm.value())
                .inner_margin(egui::Margin::symmetric(
                    theme.spacing_sm.value() as i8,
                    theme.spacing_xs.value() as i8,
                ))
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(name)
                            .size(theme.font_size_caption.value())
                            .color(egui::Color32::from(ink)),
                    );
                });
        }
    });
}

/// Mocha·Latte 한 장. 디자인 C4 행의 비율 문구(이전 값 포함)를 그대로 싣는다.
fn scroll_card(ui: &mut egui::Ui, theme: &Theme, name: &str, ratios: &str) {
    egui::Frame::new()
        .fill(egui::Color32::from(theme.bg_app()))
        .stroke(egui::Stroke::new(
            theme.border_width.value(),
            egui::Color32::from(theme.border_default()),
        ))
        .corner_radius(theme.corner_radius.value())
        .inner_margin(egui::Margin::same(theme.spacing_md.value() as i8))
        .show(ui, |ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = theme.spacing_md.value();
                ui.label(
                    egui::RichText::new(name)
                        .size(theme.font_size_caption.value())
                        .color(egui::Color32::from(theme.text_muted())),
                );
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = theme.spacing_xs.value();
                    ui.label(
                        egui::RichText::new(ratios)
                            .monospace()
                            .size(theme.font_size_micro.value())
                            .color(egui::Color32::from(theme.text_muted())),
                    );
                    scroll_strip(ui, theme);
                });
                ink_ladder(ui, theme);
            });
        });
}

/// 탭 스트립 스크롤 화살표의 enabled·disabled 잉크(디자인 Foundations › Disabled ink의 C4 행).
pub fn draw_scroll_arrows(ui: &mut egui::Ui, theme: &Theme) {
    let mocha = tasty_themes::mocha_fallback();
    let latte = crate::host_shell::latte_theme();
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        scroll_card(
            ui,
            &mocha,
            "Mocha",
            "C4 · tab-strip scroll — disabled 3.40:1 · enabled 5.65:1 (was 4.45 (n800))",
        );
        scroll_card(
            ui,
            &latte,
            "Latte",
            "C4 · tab-strip scroll — disabled 2.56:1 · enabled 3.65:1 (was 2.56 (n800))",
        );
    });

    spec::meta(
        ui,
        theme,
        &[
            ("A · target", "none — WCAG exempts disabled"),
            ("rule 1 · order", "placeholder < disabled < muted"),
            ("rule 2 · parity", "Latte remap n700 → n800"),
            (
                "B · ground",
                "n/a (no target) — report on the control's own ground",
            ),
            ("C · enabled arrow", "in scope → text-muted (3:1 non-text)"),
            ("one ink", "labels + glyphs, unchanged principle"),
            (
                "pixels",
                "Latte disabled everywhere · both themes' enabled arrow",
            ),
        ],
        &[
            TokenChip::new(
                "text-disabled",
                "Mocha n700 · Latte n800",
                theme.text_disabled().into(),
            ),
            TokenChip::new(
                "tab-scroll-arrow-fg",
                "→ text-muted",
                theme.tab_scroll_arrow_fg().into(),
            ),
            TokenChip::new(
                "tab-scroll-arrow-fg-disabled",
                "→ text-disabled",
                theme.tab_scroll_arrow_fg_disabled().into(),
            ),
            TokenChip::without_color("tab-scroll-arrow-glyph-size", "→ icon-size-xs 12"),
            TokenChip::without_color("tab-scroll-arrow-width", "→ control-height-tab 24"),
        ],
    );
    spec::dont(
        ui,
        theme,
        "Don't lift disabled to 4.5:1. In Latte that lands on text-muted and disabled stops reading as disabled. The hierarchy is the requirement; contrast is reported, not targeted.",
    );
}
