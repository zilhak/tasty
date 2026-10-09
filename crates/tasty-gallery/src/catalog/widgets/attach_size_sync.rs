//! 원격 attach mirror 의 터미널 크기 동기화 실패 배너 — 기본 · 다시 시도 중 · 여러 surface.
//! 배너는 본체와 같은 `tasty_ui_widgets` 함수로 그리고, 묶음 틀과 상태 캡션만 여기서 그린다.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{AttachSizeSyncBannerView, attach_size_sync_banner};

use crate::catalog::spec::{self, StageVariant, TokenChip};
use crate::i18n::t;

/// 테마 묶음 하나의 폭. 디자인은 `--tasty-size-460`을 쓴다.
const PANEL_W: LogicalPx = LogicalPx(460.0);

fn banner(ui: &mut egui::Ui, theme: &Theme, names: &[&str], retrying: bool) {
    attach_size_sync_banner(
        ui,
        theme,
        &AttachSizeSyncBannerView {
            title: t("remote.size_sync.title"),
            body: t("remote.size_sync.body"),
            many: t("remote.size_sync.many"),
            names,
            retry: t("remote.size_sync.retry"),
            retry_all: t("remote.size_sync.retry_all"),
            dismiss: t("remote.size_sync.dismiss"),
            retrying,
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
                banner(ui, theme, &["build"], false);
                caption(ui, theme, "retrying (\u{2264} 5 s)");
                banner(ui, theme, &["build"], true);
                caption(ui, theme, "3 surfaces \u{b7} long tab title truncated");
                banner(
                    ui,
                    theme,
                    &["release-pipeline-watch-logs-eu-west", "build", "tests"],
                    false,
                );
            });
        });
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
                "banner-body 11 · muted · one line · {names} · The remote may still be using the old size.",
            ),
            (
                "name",
                "text-secondary · each name ellipsis at attach-sync-name-max-width 160; first two names, then +n; the fixed copy is never cut",
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
        "Strings (en): remote.size_sync.title · remote.size_sync.body · remote.size_sync.many · \
         remote.size_sync.retry · remote.size_sync.retry_all. Under banner-narrow-below the action \
         group wraps under the text like every banner.",
    );
}
