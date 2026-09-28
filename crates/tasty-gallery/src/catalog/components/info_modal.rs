//! 부팅·실행 중 안내 메시지 큐의 모달 셸 예제.
//!
//! 본문과 버튼 행은 본체와 같은 `tasty_ui_widgets::info_modal`을 호출한다. 셸 크기는
//! `info-modal-*` 토큰과 `info_modal_shell_height`로 정한다. 타이틀바는 본체 팝업
//! 타이틀바(`src/adapters/ui/popup/draw.rs`)를 따라 그린다 — 채움 bg-sidebar, 가운데 제목,
//! 아래 1px 선. 안내 모달만 제목을 `font-size-max`로, 선을 `info-modal-title-edge`로 그린다.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{
    ButtonVariant, InfoModalButton, InfoModalView, info_modal, info_modal_shell_height,
};

use crate::catalog::popup_frame::{self, TITLE_BAR_HEIGHT, TitleButtons};
use crate::catalog::spec::{self, StageVariant, TokenChip};

const THEME_NOT_FOUND: &str =
    "The theme \"gruvbox-hard\" set in settings.toml was not found, so the default theme is used.";
const DB_LOCKED: &str = "The database file is locked by another Tasty process. Close the other \
                         instance and start Tasty again.";
/// 권한 안내 본문의 번역 키. 본체와 같은 문자열과 강조 표기를 `lang/en.toml`에서 읽는다.
const PERMISSIONS_NOTICE_KEY: &str = "macos_permissions.notice.body";

/// 권한 안내 본문. 갤러리는 설정을 읽지 않으므로 처음 쓸 때 영어 번역표로 한 번 초기화한다.
fn permissions_notice() -> &'static str {
    static INIT: std::sync::Once = std::sync::Once::new();
    INIT.call_once(|| {
        let report = tasty_i18n::init("en");
        if report.fell_back() {
            tracing::warn!("gallery i18n init fell back: {report:?}");
        }
    });
    tasty_i18n::t(PERMISSIONS_NOTICE_KEY)
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
        popup_frame::draw_title_buttons(&painter, theme, title_rect, TitleButtons::CLOSE);
    let pad = theme.spacing_sm.value();
    let title_avail = egui::Rect::from_min_max(
        egui::pos2(title_rect.min.x + pad, title_rect.min.y),
        egui::pos2(
            (buttons_left - pad).max(title_rect.min.x + pad),
            title_rect.max.y,
        ),
    );
    painter.with_clip_rect(title_avail).text(
        title_avail.center(),
        egui::Align2::CENTER_CENTER,
        title,
        egui::FontId::proportional(theme.font_size_max.value()),
        theme.text_primary().to_egui(),
    );

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
            ("title", "popup title bar · font-size-max 14 · rule below"),
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

/// macOS 권한 안내 — 본문이 가장 긴 경우. 강조 표기는 이 메시지만 쓴다.
pub fn draw_permissions(ui: &mut egui::Ui, theme: &Theme) {
    let latte = crate::host_shell::latte_theme();
    let cases: [(&str, &str, &Theme, f32); 4] = [
        ("perm-top", "scrolled to top", theme, 0.0),
        ("perm-mid", "mid-way", theme, 0.5),
        ("perm-end", "at the end", theme, 1.0),
        ("perm-top-latte", "top — Latte", &latte, 0.0),
    ];
    spec::stage(ui, theme, StageVariant::Column, |ui| {
        for pair in cases.chunks(2) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = theme.spacing_lg.value();
                for &(key, label, th, scroll) in pair {
                    spec::cluster(ui, th, label, |ui| {
                        modal(
                            ui,
                            th,
                            key,
                            "Some permissions are not granted",
                            permissions_notice(),
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
        ],
    );

    spec::note(
        ui,
        theme,
        "[Open permission settings] 는 모달을 닫지 않는다 — 설정 창이 따로 뜨므로 안내를 다시 \
         읽을 수 있어야 한다. 스크롤 경계는 맨 위와 중간에서만 보이고 끝에서는 사라진다.",
    );
}
