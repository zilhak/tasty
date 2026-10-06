//! 상태바 테마 칸 — sun(Latte)과 theme(Mocha) 글리프를 한 상자 크기로 그린다.
//! 시안 `layouts.jsx` 의 "Theme cell — one glyph box, both themes (settled)".
//! 두 글리프를 나란히 둔 것은 비교용이다. 제품의 칸에는 하나만 들어간다.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;

use crate::catalog::icons::{MockGlyph, SUN, THEME};
use crate::catalog::spec::{StageVariant, TokenChip, dont, meta, note, stage};

// 시안 무대의 전시 치수. 대응 토큰이 없다.
/// 무대 안쪽 여백 — `padding: 20`.
const STAGE_PAD: LogicalPx = LogicalPx(20.0);
/// 배율별 칸 사이 — `gap: 14`.
const SCALE_GAP: LogicalPx = LogicalPx(14.0);
/// 배율 라벨과 바 사이 — `gap: 5`.
const SCALE_LABEL_GAP: LogicalPx = LogicalPx(5.0);
/// 바 좌우 여백 — `padding: "0 10px"`. 본체 상태바의 좌우 여백과 같은 값이다.
const BAR_PAD_X: LogicalPx = LogicalPx(10.0);
/// 비교할 UI 배율 — `[0.85, 1, 1.2]`.
const SCALES: [(f32, &str); 3] = [
    (0.85, "UI scale ×0.85"),
    (1.0, "UI scale ×1"),
    (1.2, "UI scale ×1.2"),
];

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    let mocha = tasty_themes::mocha_fallback();
    let latte = crate::host_shell::latte_theme();
    stage(ui, theme, StageVariant::Tight, |ui| {
        egui::Frame::new()
            .fill(theme.bg_app().to_egui())
            .corner_radius(theme.corner_radius.value())
            .inner_margin(egui::Margin::same(STAGE_PAD.value() as i8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = theme.spacing_lg.value();
                for (label, th) in [("Mocha", &mocha), ("Latte", &latte)] {
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = theme.spacing_sm.value();
                        ui.label(
                            egui::RichText::new(label)
                                .size(theme.font_size_caption.value())
                                .color(theme.text_muted().to_egui()),
                        );
                        theme_box(ui, th);
                    });
                }
            });
    });

    meta(
        ui,
        theme,
        &[
            ("glyph box", "12 — statusbar-glyph-size → icon-size-xs"),
            ("per theme", "the same value — Mocha and Latte never differ"),
            ("per glyph", "the same value — sun and theme never differ"),
            (
                "UI scale",
                "rides the scale with the rest: round(12×s) = 10 / 12 / 14",
            ),
            ("bar height", "24, outside the UI scale — unchanged"),
            (
                "ink ratio",
                "83% of the box for both glyphs (10px ink at box 12)",
            ),
            (
                "optical fix",
                "asset — icons/sun.svg rays 2→3 / 22→21, diagonals to ±6.4",
            ),
            ("ink before", "sun 92% · theme 83% → Latte read +2px large"),
            ("color", "statusbar-theme-glyph"),
        ],
        &[
            TokenChip::without_color("statusbar-glyph-size", "every inline glyph in the bar"),
            TokenChip::without_color("icon-size-xs", "the semantic it aliases"),
            TokenChip::new(
                "statusbar-theme-glyph",
                "theme glyph ink",
                theme.statusbar_theme_glyph().to_egui(),
            ),
            TokenChip::without_color("statusbar-dot-size", "6 — the dot on the same line"),
        ],
    );
    note(
        ui,
        theme,
        "The two glyphs sit side by side here only to compare them. In the product the cell \
         carries exactly one: sun in Latte, theme in Mocha, with the theme name beside it.",
    );
    dont(
        ui,
        theme,
        "Don't give sun its own size token to cancel the overshoot. A size that changes with \
         theme state turns a dimension into a state, and the same overshoot would follow sun \
         into every other place it is drawn. Optical parity belongs in the glyph.",
    );
}

/// 한 테마의 상자 — 그 테마의 bg-app · border-default 위에 세 배율의 바를 나란히 둔다.
fn theme_box(ui: &mut egui::Ui, th: &Theme) {
    egui::Frame::new()
        .fill(th.bg_app().to_egui())
        .stroke(egui::Stroke::new(
            th.border_width.value(),
            th.border_default().to_egui(),
        ))
        .corner_radius(th.corner_radius.value())
        .inner_margin(egui::Margin::same(th.spacing_md.value() as i8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(SCALE_GAP.value(), SCALE_GAP.value());
                for (s, label) in SCALES {
                    let scaled =
                        Theme::with_colors_and_zoom(th.to_colors(), th.is_light, th.ui_zoom * s);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = SCALE_LABEL_GAP.value();
                        ui.label(
                            egui::RichText::new(label)
                                .monospace()
                                .size(th.font_size_micro.value())
                                .color(th.text_muted().to_egui()),
                        );
                        bar(ui, th, &scaled);
                    });
                }
            });
        });
}

/// 바 한 줄 — dot · main · sun · Latte · theme · Mocha. 글자와 글리프는 배율을 따르고
/// 바 높이와 dot 은 배율 밖이다(시안의 CSS 변수는 배율과 무관하다).
fn bar(ui: &mut egui::Ui, th: &Theme, scaled: &Theme) {
    let font = egui::FontId::proportional(scaled.font_size_caption.value());
    let ink = th.text_muted().to_egui();
    let glyph = scaled.statusbar_glyph_size().value();
    let dot = th.statusbar_dot_size().value();
    let gap = th.spacing_sm.value();
    let texts: Vec<_> = ["main", "Latte", "Mocha"]
        .iter()
        .map(|t| ui.fonts(|f| f.layout_no_wrap((*t).to_owned(), font.clone(), ink)))
        .collect();
    let text_w: f32 = texts.iter().map(|g| g.size().x).sum();
    let w = BAR_PAD_X.value() * 2.0 + dot + text_w + glyph * 2.0 + gap * 5.0;
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(w, th.status_bar_height.value()),
        egui::Sense::hover(),
    );
    ui.painter().rect(
        rect,
        th.corner_radius_sm.value(),
        th.bg_sidebar().to_egui(),
        egui::Stroke::new(th.border_width.value(), th.border_frame().to_egui()),
        egui::StrokeKind::Inside,
    );
    let cy = rect.center().y;
    let mut x = rect.left() + BAR_PAD_X.value();
    ui.painter().circle_filled(
        egui::pos2(x + dot * 0.5, cy),
        dot * 0.5,
        th.status_idle().to_egui(),
    );
    x += dot + gap;
    let mut texts = texts.into_iter();
    let mut put_text = |ui: &mut egui::Ui, x: &mut f32| {
        if let Some(g) = texts.next() {
            let w = g.size().x;
            let pos = egui::pos2(*x, cy - g.size().y * 0.5);
            ui.painter().galley(pos, g, ink);
            *x += w + gap;
        }
    };
    let put_glyph = |ui: &mut egui::Ui, x: &mut f32, icon: MockGlyph| {
        let r =
            egui::Rect::from_min_size(egui::pos2(*x, cy - glyph * 0.5), egui::vec2(glyph, glyph));
        icon.image(glyph, th.statusbar_theme_glyph().to_egui())
            .paint_at(ui, r);
        *x += glyph + gap;
    };
    put_text(ui, &mut x);
    put_glyph(ui, &mut x, SUN);
    put_text(ui, &mut x);
    put_glyph(ui, &mut x, THEME);
    put_text(ui, &mut x);
}
