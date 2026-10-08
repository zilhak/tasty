//! 부팅·실행 중 안내 메시지 큐의 모달 셸 예제.
//!
//! 본문과 버튼 행은 본체와 같은 `tasty_ui_widgets::info_modal`을 호출한다. 셸 크기는
//! `info-modal-*` 토큰과 `info_modal_shell_height`로 정한다. 타이틀바는 본체 팝업
//! 타이틀바(`src/adapters/ui/popup/draw.rs`)를 따라 그린다 — 채움 bg-sidebar, 가운데 제목,
//! `font-size-max` 제목, 아래 1px 선. 안내 모달만 선을 `info-modal-title-edge`로 그린다.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{
    ButtonVariant, InfoModalButton, InfoModalView, info_modal, info_modal_shell_height,
};

use crate::catalog::popup_frame::{self, TITLE_BAR_HEIGHT, TitleButtons};
use tasty_platform::macos_permission_notice::{FdaNoticeBranch, permission_notice_body};

use crate::catalog::spec::{self, StageVariant, TokenChip};

const THEME_NOT_FOUND: &str =
    "The theme \"gruvbox-hard\" set in settings.toml was not found, so the default theme is used.";
const DB_LOCKED: &str = "The database file is locked by another Tasty process. Close the other \
                         instance and start Tasty again.";
/// 권한 안내 본문. 본체와 같은 조립 함수로 `lang/en.toml`의 문단과 강조 표기를 잇는다.
fn permissions_notice(fda: FdaNoticeBranch) -> String {
    permission_notice_body(fda, crate::i18n::t)
}

const OK: &[InfoModalButton<'static>] = &[InfoModalButton {
    label: "OK",
    variant: ButtonVariant::Primary,
}];
const QUIT: &[InfoModalButton<'static>] = &[InfoModalButton {
    label: "Quit",
    variant: ButtonVariant::Primary,
}];
const PERMISSIONS_BUTTONS: &[InfoModalButton<'static>] = &[
    InfoModalButton {
        label: "Open permission settings",
        variant: ButtonVariant::Secondary,
    },
    InfoModalButton {
        label: "OK",
        variant: ButtonVariant::Primary,
    },
];

/// 모달 한 장. 높이는 직전 프레임에 잰 본문 높이로 정하며, 첫 프레임은 상한으로 그린다.
#[allow(clippy::too_many_arguments)] // reason: 한 장의 입력(제목·본문·강조·버튼·스크롤)을 그대로 받는다.
fn modal(
    ui: &mut egui::Ui,
    theme: &Theme,
    key: &str,
    title: &str,
    body: &str,
    emphasis: bool,
    buttons: &[InfoModalButton<'_>],
    scroll_to: Option<f32>,
) {
    let id = egui::Id::new("gallery_info_modal").with(key);
    let body_key = id.with("body_h");
    let measured = ui.ctx().data(|d| d.get_temp::<f32>(body_key));
    let h = measured.map_or(theme.info_modal_max_height(), |b| {
        info_modal_shell_height(theme, TITLE_BAR_HEIGHT, LogicalPx(b))
    });
    let (frame, _) = ui.allocate_exact_size(
        egui::vec2(theme.info_modal_width().value(), h.value()),
        egui::Sense::hover(),
    );
    let painter = ui.painter_at(frame.expand(theme.spacing_lg.value()));
    let radius = theme.corner_radius.value();
    painter.add(theme.shadow_modal().to_egui().as_shape(frame, radius));
    painter.rect_filled(frame, radius, theme.bg_panel().to_egui());
    painter.rect_stroke(
        frame,
        radius,
        egui::Stroke::new(theme.border_width.value(), theme.border_frame().to_egui()),
        egui::StrokeKind::Outside,
    );

    let title_rect = egui::Rect::from_min_size(
        frame.min,
        egui::vec2(frame.width(), TITLE_BAR_HEIGHT.value()),
    );
    let cr = radius as u8;
    painter.rect_filled(
        title_rect,
        egui::CornerRadius {
            nw: cr,
            ne: cr,
            sw: 0,
            se: 0,
        },
        theme.bg_sidebar().to_egui(),
    );
    painter.hline(
        title_rect.x_range(),
        title_rect.max.y,
        egui::Stroke::new(
            theme.border_width.value(),
            theme.info_modal_title_edge().to_egui(),
        ),
    );
    let buttons_left =
        popup_frame::draw_title_buttons(ui.ctx(), &painter, theme, title_rect, TitleButtons::CLOSE);
    let cut_band = popup_frame::draw_title_text(&painter, theme, title_rect, buttons_left, title);
    popup_frame::title_tooltip(ui, theme, title, cut_band, false);

    let content = egui::Rect::from_min_max(egui::pos2(frame.min.x, title_rect.max.y), frame.max);
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(content)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    child.set_clip_rect(content);
    let out = info_modal(
        &mut child,
        theme,
        &InfoModalView {
            id_salt: id,
            body,
            emphasis,
            buttons,
            scroll_to,
        },
    );
    ui.ctx()
        .data_mut(|d| d.insert_temp(body_key, out.body_content_height.value()));
}

fn shell_cluster(ui: &mut egui::Ui, theme: &Theme, key: &str, label: &str, db: bool) {
    spec::cluster(ui, theme, label, |ui| {
        if db {
            modal(
                ui,
                theme,
                key,
                "Database initialization error",
                DB_LOCKED,
                false,
                QUIT,
                None,
            );
        } else {
            modal(
                ui,
                theme,
                key,
                "Theme not found",
                THEME_NOT_FOUND,
                false,
                OK,
                None,
            );
        }
    });
}

/// 셸 공통 규칙 — 한 문장짜리 메시지와 앱을 끝내는 메시지.
pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    let latte = crate::host_shell::latte_theme();
    // 모달 폭 440 두 장이 문서 컬럼에 들어가므로 두 장씩 한 줄에 놓는다.
    spec::stage(ui, theme, StageVariant::Column, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = theme.spacing_lg.value();
            shell_cluster(
                ui,
                theme,
                "theme",
                "Theme not found — one sentence, min height",
                false,
            );
            shell_cluster(
                ui,
                theme,
                "db",
                "Database initialization error — quits",
                true,
            );
        });
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = theme.spacing_lg.value();
            shell_cluster(ui, &latte, "theme-latte", "Theme not found — Latte", false);
            shell_cluster(
                ui,
                &latte,
                "db-latte",
                "Database initialization error — Latte",
                true,
            );
        });
    });

    spec::meta(
        ui,
        theme,
        &[
            (
                "size",
                "info-modal-width 440 × min-height 140 .. max-height 360",
            ),
            (
                "title",
                "shared popup title bar · bg-sidebar · centred, one line · font-size-max 14 · rule below",
            ),
            (
                "close ×",
                "dismiss, same as Enter / Esc · outside click never closes",
            ),
            ("body", "13 · text-secondary · para gap 12 · plain"),
            (
                "scroll edge",
                "above buttons, only while content is hidden below",
            ),
            ("dismiss", "Primary, rightmost · Enter / Esc"),
            ("app-ending", "same Primary, label Quit"),
        ],
        &[
            TokenChip::new("bg-panel", "shell", theme.bg_panel().to_egui()),
            TokenChip::new(
                "info-modal-title-edge",
                "title bar rule",
                theme.info_modal_title_edge().to_egui(),
            ),
            TokenChip::new(
                "info-modal-scroll-edge",
                "rule above buttons",
                theme.info_modal_scroll_edge().to_egui(),
            ),
            TokenChip::new("text-secondary", "body", theme.text_secondary().to_egui()),
            TokenChip::without_color("info-modal-min-height", "→ size-140"),
        ],
    );

    spec::note(
        ui,
        theme,
        "큐 모델이다 — 여러 건이 쌓이면 닫기 버튼마다 다음 메시지로 넘어가고, 마지막을 \
         닫아야 popup 이 닫힌다. X 로 닫아도 head 를 pop 해 남은 안내가 유실되지 않는다. \
         데이터베이스 오류는 버튼이 하나뿐이고 라벨이 Quit 이다 — 누르면 앱이 끝난다.",
    );
}

/// 알림 popup 머리 — 전체화면 무대를 선언한 popup 이라 버튼이 둘(fit + ×)이다.
/// 타이틀바는 본체 popup 타이틀바와 같이 `font-size-max` 제목과 border-frame 선으로 그린다.
fn notifications_head(ui: &mut egui::Ui, theme: &Theme, title: &str) {
    let body_font = egui::FontId::proportional(theme.font_size_body.value());
    let line = ui.fonts(|f| f.row_height(&body_font));
    let body_h = theme.spacing_md.scaled(2.0) + LogicalPx(line);
    let (frame, _) = ui.allocate_exact_size(
        egui::vec2(
            theme.notifications_popup_width().value(),
            (TITLE_BAR_HEIGHT + body_h).value(),
        ),
        egui::Sense::hover(),
    );
    let painter = ui.painter_at(frame.expand(theme.spacing_lg.value()));
    let radius = theme.corner_radius.value();
    painter.rect_filled(frame, radius, theme.bg_panel().to_egui());
    painter.rect_stroke(
        frame,
        radius,
        egui::Stroke::new(theme.border_width.value(), theme.border_frame().to_egui()),
        egui::StrokeKind::Outside,
    );
    let title_rect = egui::Rect::from_min_size(
        frame.min,
        egui::vec2(frame.width(), TITLE_BAR_HEIGHT.value()),
    );
    let cr = radius as u8;
    painter.rect_filled(
        title_rect,
        egui::CornerRadius {
            nw: cr,
            ne: cr,
            sw: 0,
            se: 0,
        },
        theme.bg_sidebar().to_egui(),
    );
    painter.hline(
        title_rect.x_range(),
        title_rect.max.y,
        egui::Stroke::new(theme.border_width.value(), theme.border_frame().to_egui()),
    );
    let buttons_left = popup_frame::draw_title_buttons(
        ui.ctx(),
        &painter,
        theme,
        title_rect,
        TitleButtons::FULLSCREEN_AND_CLOSE,
    );
    let cut_band = popup_frame::draw_title_text(&painter, theme, title_rect, buttons_left, title);
    popup_frame::title_tooltip(ui, theme, title, cut_band, false);
    painter.text(
        egui::pos2(
            frame.min.x + theme.spacing_lg.value(),
            title_rect.max.y + theme.spacing_md.value(),
        ),
        egui::Align2::LEFT_TOP,
        "No notifications.",
        body_font,
        theme.text_muted().to_egui(),
    );
}

/// 공용 popup 타이틀바 — 버튼이 하나든 둘이든 제목은 스트립 가운데다.
pub fn draw_title_bar(ui: &mut egui::Ui, theme: &Theme) {
    let latte = crate::host_shell::latte_theme();
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        for (label, th) in [("Mocha", theme), ("Latte", &latte)] {
            spec::cluster(
                ui,
                th,
                &format!("{label} · two buttons · short title"),
                |ui| {
                    notifications_head(ui, th, "Notifications");
                },
            );
            spec::cluster(
                ui,
                th,
                &format!("{label} · two buttons · long title ellipsises in the symmetric band"),
                |ui| {
                    notifications_head(
                        ui,
                        th,
                        "Notifications from every workspace and remote host",
                    );
                },
            );
        }
    });

    spec::meta(
        ui,
        theme,
        &[
            (
                "title",
                "font-size-max 14 · text-primary · semibold not reproduced",
            ),
            ("centre", "strip centre, always"),
            ("reserve", "4 + N × 24 + (N − 1) × 4 + 4 → 32 · 60 per side"),
            (
                "buttons",
                "IconButton sm 24 · close / fit glyph · hover and press overlays",
            ),
            ("order", "fullscreen (fit) · close"),
            ("fullscreen", "only when the popup declares a stage"),
            ("long title", "ellipsis inside the band"),
        ],
        &[
            TokenChip::new("bg-sidebar", "title bar", theme.bg_sidebar().to_egui()),
            TokenChip::new("icon-button-fg", "glyphs", theme.icon_button_fg().to_egui()),
            TokenChip::without_color("popup-title-btn-size", "→ icon-button-size-sm 24"),
            TokenChip::without_color("popup-title-btn-gap", "→ space-xs 4"),
            TokenChip::without_color("popup-title-edge-inset", "→ space-xs 4"),
            TokenChip::without_color("popup-title-text-gap", "→ space-xs 4"),
            TokenChip::without_color("notifications-popup-width", "→ size-352"),
        ],
    );

    spec::dont(
        ui,
        theme,
        "Don't centre the title in the space left of the buttons. With two buttons the title \
         then sits 28 px left of centre, and two popups side by side no longer line up.",
    );
}

/// macOS 권한 안내 — 본문이 가장 긴 경우. 강조 표기는 이 메시지만 쓴다.
pub fn draw_permissions(ui: &mut egui::Ui, theme: &Theme) {
    let latte = crate::host_shell::latte_theme();
    let never = FdaNoticeBranch::Never;
    let cases: [(&str, &str, &Theme, f32, FdaNoticeBranch); 6] = [
        ("perm-top", "scrolled to top", theme, 0.0, never),
        ("perm-mid", "mid-way", theme, 0.5, never),
        ("perm-end", "at the end", theme, 1.0, never),
        ("perm-top-latte", "top — Latte", &latte, 0.0, never),
        (
            "perm-stale",
            "FDA granted before, Tasty changed since",
            theme,
            0.0,
            FdaNoticeBranch::Stale,
        ),
        (
            "perm-revoked",
            "FDA granted before, turned off or reset outside Tasty",
            theme,
            0.0,
            FdaNoticeBranch::Revoked,
        ),
    ];
    spec::stage(ui, theme, StageVariant::Column, |ui| {
        for pair in cases.chunks(2) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = theme.spacing_lg.value();
                for &(key, label, th, scroll, fda) in pair {
                    spec::cluster(ui, th, label, |ui| {
                        modal(
                            ui,
                            th,
                            key,
                            "Some permissions are not granted",
                            &permissions_notice(fda),
                            true,
                            PERMISSIONS_BUTTONS,
                            Some(scroll),
                        );
                    });
                }
            });
        }
    });

    spec::meta(
        ui,
        theme,
        &[
            ("height", "140..360 · body scrolls"),
            ("paths", "text-primary (medium은 색으로만)"),
            ("lead-ins", "text-primary (semibold는 색으로만)"),
            ("command", "mono · caption · surface-raised 배경"),
            (
                "buttons",
                "Open permission settings (Secondary) · OK (Primary, rightmost)",
            ),
        ],
        &[
            TokenChip::new(
                "text-primary",
                "paths · lead-ins",
                theme.text_primary().to_egui(),
            ),
            TokenChip::new(
                "surface-raised",
                "command chip",
                theme.surface_raised().to_egui(),
            ),
            TokenChip::new(
                "info-modal-scroll-edge",
                "rule above buttons",
                theme.info_modal_scroll_edge().to_egui(),
            ),
            TokenChip::without_color("info-modal-para-gap", "→ space-md 12"),
            TokenChip::without_color("info-modal-max-height", "→ size-360"),
            TokenChip::without_color("info-modal-width", "→ size-440"),
        ],
    );

    spec::note(
        ui,
        theme,
        "[Open permission settings] 는 모달을 닫지 않는다 — 설정 창이 따로 뜨므로 안내를 다시 \
         읽을 수 있어야 한다. 스크롤 경계는 맨 위와 중간에서만 보이고 끝에서는 사라진다.",
    );
}

/// 권한 안내의 FDA 갈래와 서명 보조 문단. 시안 "FDA branches · signing aside" 를 옮겼다 —
/// 갈래와 보조 문단이 함께 보이도록 네 장 모두 끝까지 스크롤한다.
pub fn draw_permission_branches(ui: &mut egui::Ui, theme: &Theme) {
    let latte = crate::host_shell::latte_theme();
    let cases: [(&str, &str, &Theme, FdaNoticeBranch); 4] = [
        (
            "perm-branch-never",
            "never — no grant on record",
            theme,
            FdaNoticeBranch::Never,
        ),
        (
            "perm-branch-stale",
            "stale — granted before, app changed",
            theme,
            FdaNoticeBranch::Stale,
        ),
        (
            "perm-branch-stale-latte",
            "stale — Latte",
            &latte,
            FdaNoticeBranch::Stale,
        ),
        (
            "perm-branch-revoked",
            "revoked — turned off outside Tasty",
            theme,
            FdaNoticeBranch::Revoked,
        ),
    ];
    spec::stage(ui, theme, StageVariant::Column, |ui| {
        for pair in cases.chunks(2) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = theme.spacing_lg.value();
                for &(key, label, th, fda) in pair {
                    spec::cluster(ui, th, label, |ui| {
                        modal(
                            ui,
                            th,
                            key,
                            "Some permissions are not granted",
                            &permissions_notice(fda),
                            true,
                            PERMISSIONS_BUTTONS,
                            Some(1.0),
                        );
                    });
                }
            });
        }
    });

    spec::meta(
        ui,
        theme,
        &[
            ("branches", "never · stale · revoked — FDA paragraph only"),
            (
                "stale steps",
                "ordered list 1–2 · body 13 · text-primary · indent space-xl · gap space-xs",
            ),
            (
                "signing aside",
                "every notice · last · caption 12 · text-muted · command chip unchanged",
            ),
            (
                "new strings",
                "stale step 1 · stale step 2 · stale retry line (split from the old stale paragraph)",
            ),
        ],
        &[
            TokenChip::new("text-muted", "signing aside", theme.text_muted().to_egui()),
            TokenChip::without_color("font-size-caption", "signing aside"),
            TokenChip::without_color("space-xl", "step list indent"),
        ],
    );
}
