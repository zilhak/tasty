//! 자동 attach 매핑을 연결하지 않았을 때의 안내 — 사이드바 행 표지와 Workspace 배너.
//! 배너와 행 표지는 본체와 같은 `tasty_ui_widgets` 함수로 그리고, 사이드바 행과 터미널 자리만 여기서 흉내 낸다.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{AttachRefusalBannerView, attach_refusal_banner, attach_refusal_mark};

use crate::catalog::spec::{self, StageVariant, TokenChip};
use crate::i18n::t;

/// 사이드바 흉내 폭. 디자인은 `--tasty-size-200`을 쓴다.
const SIDEBAR_W: LogicalPx = LogicalPx(200.0);
/// 배너와 터미널 자리를 담는 열의 폭. 디자인은 `--tasty-size-460`을 쓴다.
const CONTENT_W: LogicalPx = LogicalPx(460.0);
/// 배너 아래 터미널 자리의 높이. 디자인은 `--tasty-size-64`를 쓴다.
const TERMINAL_H: LogicalPx = LogicalPx(64.0);
/// 다른 이유 배너의 최대 폭. 디자인은 `--tasty-size-560`을 쓴다.
const OTHER_REASON_MAX_W: LogicalPx = LogicalPx(560.0);
/// 사이드바 흉내 행 높이. 디자인 `RefusalRowG`의 `--tasty-size-28`이다.
const ROW_H: LogicalPx = LogicalPx(28.0);
/// 사이드바 흉내 행 사이 간격. 디자인은 `--tasty-size-1`을 쓴다.
const ROW_GAP: LogicalPx = LogicalPx(1.0);

/// 거절 이유 하나. 대상은 디자인 예제의 값이다.
#[derive(Clone, Copy)]
enum Reason {
    SelfInstance,
    ProfileMissing,
    Unresolved,
}

impl Reason {
    fn target(self) -> &'static str {
        match self {
            Reason::SelfInstance => "127.0.0.1:7420",
            Reason::ProfileMissing => "prod-web",
            Reason::Unresolved => "build-eu:7420",
        }
    }

    fn reason_key(self) -> &'static str {
        match self {
            Reason::SelfInstance => "remote.refusal.self",
            Reason::ProfileMissing => "remote.refusal.profile_missing",
            Reason::Unresolved => "remote.refusal.unresolved",
        }
    }
}

/// 번역문 `remote.refusal.title`의 `{}` 앞뒤.
fn title_parts() -> (&'static str, &'static str) {
    t("remote.refusal.title")
        .split_once("{}")
        .unwrap_or((t("remote.refusal.title"), ""))
}

fn banner(ui: &mut egui::Ui, theme: &Theme, reason: Reason) {
    let (before, after) = title_parts();
    let body = format!("{} {}", t(reason.reason_key()), t("remote.refusal.hint"));
    attach_refusal_banner(
        ui,
        theme,
        &AttachRefusalBannerView {
            title_before: before,
            target: reason.target(),
            title_after: after,
            body: &body,
            remove: t("remote.refusal.remove"),
            dismiss: t("remote.refusal.dismiss"),
        },
    );
}

/// 디자인 `RefusalRowG` — 점 · 이름 · 끝 칸 표지.
fn row(ui: &mut egui::Ui, theme: &Theme, name: &str, active: bool, refused: bool) {
    let fill = if active {
        theme.surface_active().to_egui()
    } else {
        egui::Color32::TRANSPARENT
    };
    egui::Frame::new()
        .fill(fill)
        .corner_radius(theme.corner_radius_sm.value())
        .inner_margin(egui::Margin::symmetric(theme.spacing_sm.value() as i8, 0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_height(ROW_H.value());
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
                let dot = theme.status_dot_size().value();
                let (dot_rect, _) =
                    ui.allocate_exact_size(egui::vec2(dot, dot), egui::Sense::hover());
                ui.painter().circle_filled(
                    dot_rect.center(),
                    dot * 0.5,
                    theme.status_dot_idle().to_egui(),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if refused {
                        let tip = format!(
                            "{}\n{}",
                            t("remote.refusal.title").replacen(
                                "{}",
                                Reason::SelfInstance.target(),
                                1
                            ),
                            t(Reason::SelfInstance.reason_key())
                        );
                        attach_refusal_mark(ui, theme, &tip);
                    }
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        let color = if active {
                            theme.text_primary()
                        } else {
                            theme.text_secondary()
                        };
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(name)
                                    .size(theme.font_size_body.value())
                                    .color(color),
                            )
                            .truncate(),
                        );
                    });
                });
            });
        });
}

/// 한 테마의 사이드바 + 배너 + 터미널 자리 묶음.
fn panel(ui: &mut egui::Ui, theme: &Theme) {
    egui::Frame::new()
        .fill(theme.bg_app().to_egui())
        .stroke(egui::Stroke::new(
            theme.border_width.value(),
            theme.border_default().to_egui(),
        ))
        .corner_radius(theme.corner_radius.value())
        .inner_margin(egui::Margin::same(theme.spacing_md.value() as i8))
        .show(ui, |ui| {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = theme.spacing_md.value();
                egui::Frame::new()
                    .fill(theme.bg_sidebar().to_egui())
                    .corner_radius(theme.corner_radius.value())
                    .inner_margin(egui::Margin::same(theme.spacing_xs.value() as i8))
                    .show(ui, |ui| {
                        let inner = SIDEBAR_W.value() - 2.0 * theme.spacing_xs.value();
                        ui.set_width(inner);
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = ROW_GAP.value();
                            row(ui, theme, "tasty-core", false, false);
                            row(ui, theme, "staging-mirror", true, true);
                            row(ui, theme, "scratch", false, false);
                        });
                    });
                ui.vertical(|ui| {
                    ui.set_width(CONTENT_W.value());
                    ui.spacing_mut().item_spacing.y = theme.spacing_sm.value();
                    banner(ui, theme, Reason::SelfInstance);
                    egui::Frame::new()
                        .fill(egui::Color32::from(theme.surface("terminal").focused_bg))
                        .corner_radius(theme.corner_radius.value())
                        .inner_margin(egui::Margin::same(theme.spacing_sm.value() as i8))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.set_height(TERMINAL_H.value() - 2.0 * theme.spacing_sm.value());
                            ui.label(
                                egui::RichText::new("local terminal — still usable")
                                    .monospace()
                                    .size(theme.font_size_caption.value())
                                    .color(theme.text_muted().to_egui()),
                            );
                        });
                });
            });
        });
}

/// 시안 "Auto-attach refused": 행 표지와 Workspace 배너를 두 테마로, 다른 이유 두 가지를 아래에 보인다.
pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    let latte = crate::host_shell::latte_theme();
    let mocha = tasty_themes::mocha_fallback();
    spec::stage(ui, theme, StageVariant::Tight, |ui| {
        egui::Frame::new()
            .fill(theme.bg_app().to_egui())
            .inner_margin(egui::Margin::same(theme.spacing_lg.value() as i8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                // 디자인 Stage는 flex-wrap이다. 문서 폭에 두 묶음이 나란히 들어가지 않아 세로로 쌓인다.
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = theme.spacing_lg.value();
                    panel(ui, &mocha);
                    panel(ui, &latte);
                });
            });
    });
    spec::stage(ui, theme, StageVariant::Tight, |ui| {
        egui::Frame::new()
            .fill(theme.bg_app().to_egui())
            .inner_margin(egui::Margin::same(theme.spacing_lg.value() as i8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = theme.spacing_sm.value();
                ui.label(
                    egui::RichText::new("other mapping errors — same banner, own reason")
                        .size(theme.font_size_caption.value())
                        .color(theme.text_muted().to_egui()),
                );
                for reason in [Reason::ProfileMissing, Reason::Unresolved] {
                    ui.scope(|ui| {
                        ui.set_max_width(OTHER_REASON_MAX_W.value());
                        banner(ui, theme, reason);
                    });
                }
            });
    });
    spec::meta(
        ui,
        theme,
        &[
            (
                "row mark",
                "alertTriangle 14 · accent-warning · trailing slot of the workspace row · tooltip = target + reason",
            ),
            (
                "banner scope",
                "Workspace · shown while that workspace is active",
            ),
            ("glyph", "alertTriangle 16 · accent-warning"),
            ("title", "Remote not attached — {target} (target mono)"),
            (
                "body",
                "{reason} Change or remove the mapping for this workspace.",
            ),
            (
                "action",
                "Remove mapping · banner button (secondary sm on banner-button tokens)",
            ),
            ("×", "hides for this activation; the row mark stays"),
            (
                "clears",
                "mapping changed / removed · profile re-check succeeds",
            ),
            ("focus", "never moved — agent-made mappings included"),
            ("toast", "none"),
        ],
        &[
            TokenChip::new(
                "accent-warning",
                "glyph + row mark",
                theme.accent_warning().to_egui(),
            ),
            TokenChip::new("banner-bg", "shell", theme.banner_bg().to_egui()),
            TokenChip::new(
                "banner-button-bg",
                "Remove mapping",
                theme.banner_button_bg().to_egui(),
            ),
            TokenChip::without_color("banner-body-font-size", "reason 11"),
        ],
    );
    spec::note(
        ui,
        theme,
        "Strings (en): remote.refusal.title · remote.refusal.self · remote.refusal.profile_missing · \
         remote.refusal.unresolved · remote.refusal.hint · remote.refusal.remove. No new tokens.",
    );
}
