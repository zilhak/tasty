//! 네이티브 WebView 영역의 경계와 로딩·오류 안내 예제.
//! 갤러리는 웹 페이지를 띄우지 않고 주변 UI만 Theme 값으로 그린다.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::Spinner;

use crate::catalog::icons;
use crate::catalog::spec::{self, StageVariant, TokenChip};

/// chrome 타일 폭.
const TILE_W: LogicalPx = LogicalPx(240.0);
/// chrome 타일 높이.
const TILE_H: LogicalPx = LogicalPx(150.0);

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        spec::cluster(ui, theme, "boundary — webview region", |ui| {
            tile(ui, theme, |ui| {
                glyph(
                    ui,
                    icons::HTML,
                    theme.icon_glyph_size_md.value(),
                    theme.text_muted(),
                );
                gap(ui, theme);
                label(ui, theme, "WebView region", theme.text_muted());
                label(ui, theme, "https://tasty.dev", theme.text_disabled());
            });
        });
        spec::cluster(ui, theme, "placeholder — no URL", |ui| {
            tile(ui, theme, |ui| {
                glyph(
                    ui,
                    icons::HTML,
                    theme.icon_glyph_size_md.value(),
                    theme.text_disabled(),
                );
                gap(ui, theme);
                label(ui, theme, "No page loaded", theme.text_muted());
            });
        });
        spec::cluster(ui, theme, "loading", |ui| {
            tile(ui, theme, |ui| {
                Spinner::new()
                    .size(theme.spinner_size.value())
                    .show(ui, theme);
                gap(ui, theme);
                label(ui, theme, "Loading…", theme.text_muted());
            });
        });
        spec::cluster(ui, theme, "error — load failed", |ui| {
            tile(ui, theme, |ui| {
                glyph(
                    ui,
                    icons::ALERT_CIRCLE,
                    theme.icon_glyph_size_md.value(),
                    theme.accent_danger(),
                );
                gap(ui, theme);
                label(ui, theme, "Failed to load", theme.accent_danger());
                label(ui, theme, "https://tasty.dev", theme.text_disabled());
            });
        });
    });

    spec::meta(
        ui,
        theme,
        &[
            ("kind", "rendering = webview · OS overlay"),
            ("content", "native WebView — token-irrelevant"),
            ("chrome", "tile boundary + state placeholder"),
            ("loading", "Spinner (ui-widgets)"),
            ("error", "alertCircle · accent-danger"),
            ("frame", "bg-panel · 1px border-default"),
        ],
        &[
            TokenChip::new("bg-panel", "tile", theme.bg_panel().to_egui()),
            TokenChip::new(
                "border-default",
                "boundary",
                theme.border_default().to_egui(),
            ),
            TokenChip::new("text-muted", "captions", theme.text_muted().to_egui()),
            TokenChip::new("accent-danger", "error", theme.accent_danger().to_egui()),
        ],
    );

    spec::note(
        ui,
        theme,
        "An HTML surface mounts a native OS WebView overlay, so the page pixels are not \
         tasty's to theme — only the chrome is. This specimen is deliberately thin: the \
         tile boundary where the overlay attaches, plus the placeholder / loading / error \
         states the host paints before or instead of a live page. The content region is \
         left empty because the overlay covers it.",
    );
}

/// 고정 W×H 테두리 타일, 콘텐츠를 상단에서 가운데 정렬로 쌓는다.
fn tile(ui: &mut egui::Ui, theme: &Theme, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(theme.bg_panel().to_egui())
        .stroke(egui::Stroke::new(
            theme.border_width.value(),
            theme.border_default().to_egui(),
        ))
        .corner_radius(theme.corner_radius.value())
        .show(ui, |ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(TILE_W.value(), TILE_H.value()),
                egui::Layout::top_down(egui::Align::Center),
                |ui| {
                    ui.add_space(theme.spacing_xl.value() * 2.0);
                    add(ui);
                },
            );
        });
}

fn glyph(ui: &mut egui::Ui, g: icons::MockGlyph, size: f32, color: impl Into<egui::Color32>) {
    ui.add(g.image(size, color.into()));
}

fn label(ui: &mut egui::Ui, theme: &Theme, text: &str, color: impl Into<egui::Color32>) {
    ui.label(
        egui::RichText::new(text)
            .size(theme.font_size_body.value())
            .color(color.into()),
    );
}

fn gap(ui: &mut egui::Ui, theme: &Theme) {
    ui.add_space(theme.spacing_sm.value());
}
