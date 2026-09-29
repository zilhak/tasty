//! 페인 탭 스트립 시안(`gallery/layouts-tabstrip.jsx`)의 스크롤 화살표 예제.
//! 스트립·탭 칸·화살표는 정적 데이터로 그린다. 본체의 스크롤 계산은 실행하지 않는다.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;

use crate::catalog::spec::{self, StageVariant, TokenChip};

/// 시안 스트립 폭(`--tasty-size-560`). 무대가 좁으면 가용 폭으로 줄인다.
const STRIP_W: LogicalPx = LogicalPx(560.0);
/// 시안 `Strip`의 기본 탭 다섯 개. 두 번째 탭이 활성이다.
const STRIP_TABS: [&str; 5] = ["server", "dev", "vim", "logs", "tests"];

/// 스트립이 어디까지 스크롤됐는지. 시안 `at`.
#[derive(Clone, Copy, PartialEq)]
enum At {
    Start,
    Mid,
    End,
}

#[derive(Clone, Copy, PartialEq)]
enum Side {
    Left,
    Right,
}

#[derive(Clone, Copy)]
struct StripCfg {
    focused: bool,
    at: At,
    hover: Option<Side>,
}

fn c(h: impl Into<egui::Color32>) -> egui::Color32 {
    h.into()
}

fn caption(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .monospace()
            .size(theme.font_size_micro.value())
            .color(c(theme.text_muted())),
    );
}

/// 시안 `Themed`: 테마 이름과 예제 줄을 담는 bg-app 카드.
fn themed(ui: &mut egui::Ui, theme: &Theme, name: &str, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(c(theme.bg_app()))
        .stroke(egui::Stroke::new(
            theme.border_width.value(),
            c(theme.border_default()),
        ))
        .corner_radius(theme.corner_radius.value())
        .inner_margin(egui::Margin::same(theme.spacing_md.value() as i8))
        .show(ui, |ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = theme.spacing_md.value();
                ui.label(
                    egui::RichText::new(name)
                        .size(theme.font_size_caption.value())
                        .color(c(theme.text_muted())),
                );
                add(ui);
            });
        });
}

/// 시안 `Arrow`: 자체 채움 없는 정사각 칸. hover 채움은 enabled 쪽에만 깐다.
fn arrow(ui: &egui::Ui, theme: &Theme, rect: egui::Rect, side: Side, disabled: bool, hover: bool) {
    if hover && !disabled {
        ui.painter().rect_filled(
            rect,
            0.0,
            theme.tab_scroll_arrow_hover_bg().to_egui_premultiplied(),
        );
    }
    let ink = if disabled {
        theme.tab_scroll_arrow_fg_disabled()
    } else {
        theme.tab_scroll_arrow_fg()
    };
    let glyph = match side {
        Side::Left => tasty_icons::CHEVRON_LEFT,
        Side::Right => tasty_icons::CHEVRON_RIGHT,
    };
    let size = theme.tab_scroll_arrow_glyph_size().value();
    glyph.image(size, c(ink)).paint_at(
        ui,
        egui::Rect::from_center_size(rect.center(), egui::vec2(size, size)),
    );
}

/// 시안 `TabCellS`의 스트립용 최소형: 아이콘·제목과 닫기 칸(활성일 때만 보임).
fn tab_cell(ui: &egui::Ui, theme: &Theme, rect: egui::Rect, label: &str, active: bool) {
    let p = ui.painter();
    let (bg, fg) = if active {
        (theme.tab_bg_active(), theme.tab_fg_active())
    } else {
        (theme.tab_bg(), theme.tab_fg())
    };
    p.rect_filled(rect, 0.0, c(bg));
    let sep = theme.border_width.value();
    p.rect_filled(
        egui::Rect::from_min_max(egui::pos2(rect.max.x - sep, rect.min.y), rect.max),
        0.0,
        theme.tab_separator().to_egui_premultiplied(),
    );
    if active {
        let bar = egui::Rect::from_min_size(
            rect.min,
            egui::vec2(rect.width(), theme.tab_indicator_width().value()),
        );
        p.rect_filled(bar, 0.0, c(theme.tab_indicator()));
    }
    let gap = theme.tab_gap().value();
    let icon = theme.tab_icon_size().value();
    let icon_rect = egui::Rect::from_min_size(
        egui::pos2(
            rect.min.x + theme.tab_padding_x().value(),
            rect.center().y - icon * 0.5,
        ),
        egui::vec2(icon, icon),
    );
    tasty_icons::HTML.image(icon, c(fg)).paint_at(ui, icon_rect);
    let close = theme.tab_close_size().value();
    let close_rect = egui::Rect::from_min_size(
        egui::pos2(
            rect.max.x - theme.spacing_xs.value() - close,
            rect.center().y - close * 0.5,
        ),
        egui::vec2(close, close),
    );
    if active {
        let x = theme.icon_glyph_size_xs.value();
        tasty_icons::CLOSE.image(x, c(theme.text_muted())).paint_at(
            ui,
            egui::Rect::from_center_size(close_rect.center(), egui::vec2(x, x)),
        );
    }
    let text_x = icon_rect.max.x + gap;
    let clip = egui::Rect::from_min_max(
        egui::pos2(text_x, rect.min.y),
        egui::pos2(close_rect.min.x - gap, rect.max.y),
    )
    .intersect(ui.clip_rect());
    ui.painter_at(clip).text(
        egui::pos2(text_x, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(theme.font_size_caption.value()),
        c(fg),
    );
}

/// 시안 `Strip`: 양끝 화살표 사이의 뷰포트에 탭 다섯 개를 `at` 위치로 스크롤해 그린다.
fn strip(ui: &mut egui::Ui, theme: &Theme, cfg: StripCfg) {
    let h = theme.tab_height().value();
    let w = STRIP_W.value().min(ui.available_width());
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
    let ground = if cfg.focused {
        theme.surface_raised()
    } else {
        theme.bg_sidebar()
    };
    ui.painter().rect_filled(rect, 0.0, c(ground));

    let aw = theme.tab_scroll_arrow_width().value();
    let left = egui::Rect::from_min_size(rect.min, egui::vec2(aw, h));
    let right =
        egui::Rect::from_min_size(egui::pos2(rect.max.x - aw, rect.min.y), egui::vec2(aw, h));
    let viewport =
        egui::Rect::from_min_max(egui::pos2(left.max.x, rect.min.y), right.left_bottom());
    let tab_w = theme.tab_width.value();
    let max_scroll = (tab_w * STRIP_TABS.len() as f32 - viewport.width()).max(0.0);
    let scroll = match cfg.at {
        At::Start => 0.0,
        At::Mid => (max_scroll * 0.5).round(),
        At::End => max_scroll,
    };
    let mut tabs = ui.new_child(egui::UiBuilder::new().max_rect(viewport));
    tabs.set_clip_rect(viewport.intersect(ui.clip_rect()));
    for (i, label) in STRIP_TABS.iter().enumerate() {
        let cell = egui::Rect::from_min_size(
            egui::pos2(viewport.min.x - scroll + i as f32 * tab_w, rect.min.y),
            egui::vec2(tab_w, h),
        );
        if cell.intersects(viewport) {
            tab_cell(&tabs, theme, cell, label, i == 1);
        }
    }

    arrow(
        ui,
        theme,
        left,
        Side::Left,
        cfg.at == At::Start,
        cfg.hover == Some(Side::Left),
    );
    arrow(
        ui,
        theme,
        right,
        Side::Right,
        cfg.at == At::End,
        cfg.hover == Some(Side::Right),
    );
    ui.painter().hline(
        rect.x_range(),
        rect.max.y - theme.border_width.value() * 0.5,
        egui::Stroke::new(theme.border_width.value(), c(theme.separator)),
    );
}

fn strip_card(ui: &mut egui::Ui, theme: &Theme, name: &str, rows: &[(&str, StripCfg)]) {
    themed(ui, theme, name, |ui| {
        for (label, cfg) in rows {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = theme.spacing_xs.value();
                caption(ui, theme, label);
                strip(ui, theme, *cfg);
            });
        }
    });
}

const SHAPE_ROWS: [(&str, StripCfg); 5] = [
    (
        "focused · left end",
        StripCfg {
            focused: true,
            at: At::Start,
            hover: None,
        },
    ),
    (
        "focused · middle · hover right",
        StripCfg {
            focused: true,
            at: At::Mid,
            hover: Some(Side::Right),
        },
    ),
    (
        "focused · right end",
        StripCfg {
            focused: true,
            at: At::End,
            hover: None,
        },
    ),
    (
        "unfocused · middle",
        StripCfg {
            focused: false,
            at: At::Mid,
            hover: None,
        },
    ),
    (
        "unfocused · left end · hover left (no fill: disabled)",
        StripCfg {
            focused: false,
            at: At::Start,
            hover: Some(Side::Left),
        },
    ),
];

/// 스크롤 화살표 모양: chevron 아이콘, 스트립 높이와 같은 정사각 칸, 자체 채움 없음.
pub fn draw_scroll_shape(ui: &mut egui::Ui, theme: &Theme) {
    let mocha = tasty_themes::mocha_fallback();
    let latte = crate::host_shell::latte_theme();
    spec::stage(ui, theme, StageVariant::Solo, |ui| {
        strip_card(ui, &mocha, "Mocha", &SHAPE_ROWS);
        strip_card(ui, &latte, "Latte", &SHAPE_ROWS);
    });
    spec::meta(
        ui,
        theme,
        &[
            ("glyph", "chevronLeft / chevronRight · 12"),
            ("cell", "24 × 24, no fill"),
            ("hover", "overlay-hover, enabled side only"),
            ("disabled", "the end reached · no hover · no response"),
            ("scope", "pane strip + horizontal_tab_bar_with_arrows"),
            ("3:1 basis", "flat colour (Mocha 5.65 · Latte 3.65)"),
        ],
        &[
            TokenChip::new(
                "tab-scroll-arrow-width",
                "→ control-height-tab 24",
                egui::Color32::TRANSPARENT,
            ),
            TokenChip::new(
                "tab-scroll-arrow-glyph-size",
                "→ icon-size-xs 12",
                egui::Color32::TRANSPARENT,
            ),
            TokenChip::new(
                "tab-scroll-arrow-fg",
                "→ text-muted",
                c(theme.tab_scroll_arrow_fg()),
            ),
            TokenChip::new(
                "tab-scroll-arrow-fg-disabled",
                "→ text-disabled",
                c(theme.tab_scroll_arrow_fg_disabled()),
            ),
        ],
    );
    spec::dont(
        ui,
        theme,
        "Don't draw arrows with a font glyph. A proportional < at 11px renders thinner than the icon stroke and depends on the UI font.",
    );
}
