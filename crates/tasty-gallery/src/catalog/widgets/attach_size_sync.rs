//! 원격 attach mirror 의 터미널 크기 동기화 실패 배너 — 기본 · 다시 시도 중 · 여러 surface.
//! 배너는 본체와 같은 `tasty_ui_widgets` 함수로 그리고, 묶음 틀과 상태 캡션만 여기서 그린다.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{AttachSizeSyncBannerView, attach_size_sync_banner, banner_is_narrow};

use crate::catalog::spec::{self, StageVariant, TokenChip};
use crate::i18n::t;

/// 테마 묶음 하나의 폭. 디자인은 `--tasty-size-460`을 쓴다. 이 폭은 시안 Stage 의 전시 치수라
/// 값만 같은 `measure-lg` 토큰을 쓰지 않는다.
const PANEL_W: LogicalPx = LogicalPx(460.0);
/// 좁은 스코프 예제의 폭. 디자인의 좁은 surface 예제(html 스크립트 배너)와 같은 `--tasty-size-360`이며
/// `banner-narrow-below`(440)보다 좁다.
const NARROW_SCOPE_W: LogicalPx = LogicalPx(360.0);

/// `scope_width`는 배너가 놓인 스코프의 폭이며 본체처럼 이 폭으로 좁은 배치를 판정한다.
fn banner(
    ui: &mut egui::Ui,
    theme: &Theme,
    names: &[&str],
    retrying: bool,
    scope_width: LogicalPx,
) {
    attach_size_sync_banner(
        ui,
        theme,
        &AttachSizeSyncBannerView {
            title: t("remote.size_sync.title"),
            names_line: t("remote.size_sync.names"),
            hint: t("remote.size_sync.hint"),
            many: t("remote.size_sync.many"),
            names,
            retry: t("remote.size_sync.retry"),
            retry_all: t("remote.size_sync.retry_all"),
            dismiss: t("remote.size_sync.dismiss"),
            retrying,
            narrow: banner_is_narrow(scope_width.value(), theme),
        },
    );
}

fn caption(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .size(theme.font_size_caption.value())
            .color(theme.text_muted().to_egui()),
    );
}

/// 디자인 Stage 의 테마 묶음 — 기본, 다시 시도 중, 세 surface 를 차례로 쌓는다.
fn panel(ui: &mut egui::Ui, theme: &Theme, label: &str) {
    egui::Frame::new()
        .fill(theme.bg_app().to_egui())
        .stroke(egui::Stroke::new(
            theme.border_width.value(),
            theme.border_default().to_egui(),
        ))
        .corner_radius(theme.corner_radius.value())
        .inner_margin(egui::Margin::same(theme.spacing_md.value() as i8))
        .show(ui, |ui| {
            ui.set_width(PANEL_W.value() - 2.0 * theme.spacing_md.value());
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = theme.spacing_sm.value();
                caption(ui, theme, &format!("{label} \u{2014} default"));
                banner(ui, theme, &["build"], false, PANEL_W);
                caption(ui, theme, "retrying (\u{2264} 5 s)");
                banner(ui, theme, &["build"], true, PANEL_W);
                caption(ui, theme, "3 surfaces \u{b7} long tab title truncated");
                banner(
                    ui,
                    theme,
                    &["release-pipeline-watch-logs-eu-west", "build", "tests"],
                    false,
                    PANEL_W,
                );
            });
        });
}

/// 시안 Narrow Spec 의 테마 묶음 — 360 스코프에 거절 · 한 surface · Retry all · 다시 시도 중을 쌓는다.
fn narrow_panel(ui: &mut egui::Ui, theme: &Theme, label: &str) {
    const MANY: [&str; 3] = ["release-pipeline-watch-logs-eu-west", "build", "tests"];
    egui::Frame::new()
        .fill(theme.bg_app().to_egui())
        .stroke(egui::Stroke::new(
            theme.border_width.value(),
            theme.border_default().to_egui(),
        ))
        .corner_radius(theme.corner_radius.value())
        .inner_margin(egui::Margin::same(theme.spacing_md.value() as i8))
        .show(ui, |ui| {
            ui.set_width(NARROW_SCOPE_W.value() - 2.0 * theme.spacing_md.value());
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = theme.spacing_sm.value();
                caption(ui, theme, &format!("{label} \u{2014} refused \u{b7} 360"));
                super::attach_refusal::refused_sample(ui, theme, NARROW_SCOPE_W);
                caption(ui, theme, "size sync \u{b7} one surface");
                banner(ui, theme, &["build"], false, NARROW_SCOPE_W);
                caption(ui, theme, "size sync \u{b7} 3 surfaces \u{b7} Retry all");
                banner(ui, theme, &MANY, false, NARROW_SCOPE_W);
                caption(ui, theme, "size sync \u{b7} retrying");
                banner(ui, theme, &MANY, true, NARROW_SCOPE_W);
            });
        });
}

/// 시안 "Narrow — action under the text, × stays top-right": 360 표본을 Mocha·Latte 로 보인다.
fn draw_narrow(ui: &mut egui::Ui, theme: &Theme) {
    spec::spec(
        ui,
        theme,
        "Narrow \u{2014} action under the text, \u{d7} stays top-right",
        Some(
            "Every banner judges narrow on the content width of its scope, before the banner \
             margins: a surface banner on that surface, a Pane / Tab banner on that pane / tab \
             content, a Workspace banner on the union of panes under the tab bar. Under \
             banner-narrow-below (440) the action button moves to its own line under the text, \
             starting at the body's left edge (glyph 16 + banner-gap), banner-gap above it. The \
             \u{d7} is a dismiss affordance, not an action: it stays in the top-right of the card. \
             Retry all and the retrying state look the same on the wrapped line. Samples at 360.",
        ),
    );
    let latte = crate::host_shell::latte_theme();
    let mocha = tasty_themes::mocha_fallback();
    spec::stage(ui, theme, StageVariant::Tight, |ui| {
        egui::Frame::new()
            .fill(theme.bg_app().to_egui())
            .inner_margin(egui::Margin::same(theme.spacing_lg.value() as i8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing =
                        egui::vec2(theme.spacing_lg.value(), theme.spacing_lg.value());
                    narrow_panel(ui, &mocha, "Mocha");
                    narrow_panel(ui, &latte, "Latte");
                });
            });
    });
    spec::meta(
        ui,
        theme,
        &[
            (
                "judged on",
                "scope content width before margins \u{b7} workspace = pane union under the tab bar",
            ),
            ("threshold", "banner-narrow-below 440"),
            (
                "action",
                "own line \u{b7} left = body edge (glyph 16 + banner-gap) \u{b7} banner-gap above",
            ),
            (
                "\u{d7}",
                "stays top-right of the card (not part of the wrapped group)",
            ),
            ("retrying / Retry all", "same on the wrapped line"),
            ("sample width", "360"),
        ],
        &[
            TokenChip::without_color("banner-narrow-below", "\u{2192} size-440"),
            TokenChip::without_color(
                "banner-gap",
                "glyph \u{2194} text \u{b7} text \u{2194} action line",
            ),
        ],
    );
}

/// 시안 "Attach mirror — terminal size sync failed": 세 상태를 Mocha·Latte 로 보인다.
pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    let latte = crate::host_shell::latte_theme();
    let mocha = tasty_themes::mocha_fallback();
    spec::stage(ui, theme, StageVariant::Tight, |ui| {
        egui::Frame::new()
            .fill(theme.bg_app().to_egui())
            .inner_margin(egui::Margin::same(theme.spacing_lg.value() as i8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing =
                        egui::vec2(theme.spacing_lg.value(), theme.spacing_lg.value());
                    panel(ui, &mocha, "Mocha");
                    panel(ui, &latte, "Latte");
                });
            });
    });
    spec::meta(
        ui,
        theme,
        &[
            (
                "boxes",
                "glyph 16 · text column (flex 1) · action group (Retry + ×, gap 4)",
            ),
            (
                "glyph",
                "alertTriangle 16 · attach-sync-glyph → accent-warning (same as refusal)",
            ),
            ("title", "banner-title 13 · semibold · never truncated"),
            (
                "body",
                "banner-body 11 · muted · two lines: ① names — one line ② “The remote may still be using the old size.” — wraps, never cut",
            ),
            (
                "name",
                "text-secondary · each name ≤ attach-sync-name-max-width 160, shrinks to fit the line down to 40 (size-40), then ellipsis; first two names, then +n; separators and +n never shrink",
            ),
            (
                "retrying",
                "Retry disabled + leading Spinner 14, label unchanged",
            ),
            ("many", "one card · “N surfaces — a, b +n” · Retry all"),
            (
                "×",
                "IconButton sm · hides this card; a new failure shows it again",
            ),
            (
                "clears",
                "Retry success · later resize success · surface closed",
            ),
            ("priority", "reconnect notice wins the slot"),
        ],
        &[
            TokenChip::new(
                "attach-sync-glyph",
                "→ accent-warning",
                theme.attach_sync_glyph().to_egui(),
            ),
            TokenChip::without_color("attach-sync-name-max-width", "→ size-160"),
            TokenChip::new(
                "banner-button-bg",
                "Retry",
                theme.banner_button_bg().to_egui(),
            ),
            TokenChip::new(
                "spinner-indicator",
                "retrying",
                theme.spinner_indicator().to_egui(),
            ),
        ],
    );
    spec::note(
        ui,
        theme,
        "Strings (en): remote.size_sync.title · remote.size_sync.names (line 1) · \
         remote.size_sync.hint (line 2) · remote.size_sync.many · remote.size_sync.retry · \
         remote.size_sync.retry_all.",
    );
    draw_narrow(ui, theme);
}
