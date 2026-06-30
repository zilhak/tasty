//! `html_chrome` specimen — HTML(webview) surface 의 host chrome (Layouts).
//!
//! HTML surface 는 `rendering = "webview"` kind 로, host 가 OS-level **native WebView
//! overlay** 를 surface 위에 붙인다(`src/engine/surface_registry/webview_kind.rs`,
//! `src/host_api/webview/*`). 실제 페이지 픽셀은 OS WebView 가 그리므로 **콘텐츠는
//! 토큰 무관** — tasty 가 토큰으로 책임지는 것은 overlay 가 붙기 전/실패 시의 *chrome*
//! 뿐이다. 따라서 이 specimen 은 얇게 chrome 상태만 전사한다:
//!
//! - **boundary** — overlay 가 마운트되는 타일 경계(테두리 + web view 영역 표식).
//! - **placeholder** — URL 미지정(navigation 전) 안내.
//! - **loading** — overlay attach·탐색 중 spinner.
//! - **error** — 로드 실패(`accent_danger` + alert glyph).
//!
//! 콘텐츠 영역 자체는 비워둔다(네이티브 overlay 가 덮음). 색·치수·폰트는 전부 `Theme`.

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::Spinner;

use crate::catalog::icons;
use crate::catalog::spec::{self, StageVariant, TokenChip};

// ── chrome 타일 대표 치수 (콘텐츠는 OS overlay — 경계 박스만 전시) ──
/// chrome 타일 폭.
const TILE_W: f32 = 240.0;
/// chrome 타일 높이.
const TILE_H: f32 = 150.0;

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        spec::cluster(ui, theme, "boundary — webview region", |ui| {
            tile(ui, theme, |ui| {
                glyph(
                    ui,
                    icons::GLOBE,
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
                    icons::GLOBE,
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
                egui::vec2(TILE_W, TILE_H),
                egui::Layout::top_down(egui::Align::Center),
                |ui| {
                    // 콘텐츠 블록을 세로 가운데쯤에 오도록 위쪽 여백.
                    ui.add_space(theme.spacing_xl.value() * 2.0);
                    add(ui);
                },
            );
        });
}

/// 중앙 정렬 glyph(정사각 tint).
fn glyph(ui: &mut egui::Ui, g: icons::MockGlyph, size: f32, color: impl Into<egui::Color32>) {
    ui.add(g.image(size, color.into()));
}

/// 캡션 한 줄(body · 지정색).
fn label(ui: &mut egui::Ui, theme: &Theme, text: &str, color: impl Into<egui::Color32>) {
    ui.label(
        egui::RichText::new(text)
            .size(theme.font_size_body.value())
            .color(color.into()),
    );
}

/// 글리프와 라벨 사이 간격.
fn gap(ui: &mut egui::Ui, theme: &Theme) {
    ui.add_space(theme.spacing_sm.value());
}
