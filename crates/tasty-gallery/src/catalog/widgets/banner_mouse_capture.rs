//! 마우스 캡처 배너의 상태 예제. 시안 `overlays-banners.jsx` 의 "Anatomy & states" 와
//! 더보기 메뉴의 "Elastic width". 첫 카드는 직전 프레임의 카드 영역에 포인터가 있으면 ⋯/× 를 드러낸다.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::banner_shell;

use super::banner::{
    MoreTriggerState, caption_label, mouse_capture_banner_body, mouse_capture_menu_row_tone,
};
use crate::catalog::icons;
use crate::catalog::spec::{self, StageVariant, TokenChip};

// 시안 무대의 전시 치수. 대응 토큰이 없다.
/// 무대 안쪽 여백 — `padding: 20`.
const STAGE_PAD: LogicalPx = LogicalPx(20.0);
/// 예제 칸 최대 폭 — `maxWidth: 480`.
const CARD_MAX_W: LogicalPx = LogicalPx(480.0);
/// 탄력 폭 예제 칸 사이 — `gap: 18`.
const ELASTIC_GAP: LogicalPx = LogicalPx(18.0);
/// 탄력 폭 예제 라벨과 메뉴 사이 — `gap: 6`.
const ELASTIC_LABEL_GAP: LogicalPx = LogicalPx(6.0);
/// 기각된 기록용 메뉴의 흐림 — `opacity: 0.75`.
const REJECTED_OPACITY: f32 = 0.75;
/// 메뉴 행의 고정 문구 — 시안 `BannerMoreMenuG` 의 행 문구.
const SUPPRESS_PREFIX: &str = "Turn off this notice for ";
const DISABLE_PREFIX: &str = "Disable mouse capture for ";

/// bg-app 무대 — 가운데 정렬 세로 묶음.
fn app_stage(ui: &mut egui::Ui, theme: &Theme, add: impl FnOnce(&mut egui::Ui)) {
    spec::stage(ui, theme, StageVariant::Tight, |ui| {
        egui::Frame::new()
            .fill(theme.bg_app().to_egui())
            .corner_radius(theme.corner_radius.value())
            .inner_margin(egui::Margin::same(STAGE_PAD.value() as i8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.vertical_centered(add);
            });
    });
}

/// 라벨 + 480 폭 카드 한 칸.
fn card_slot(ui: &mut egui::Ui, theme: &Theme, label: &str, add: impl FnOnce(&mut egui::Ui)) {
    let w = CARD_MAX_W.value().min(ui.available_width());
    ui.allocate_ui_with_layout(
        egui::vec2(w, 0.0),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            ui.set_width(w);
            ui.spacing_mut().item_spacing.y = theme.spacing_sm.value();
            caption_label(ui, theme, label);
            add(ui);
        },
    );
}

pub fn draw_anatomy(ui: &mut egui::Ui, theme: &Theme) {
    app_stage(ui, theme, |ui| {
        ui.spacing_mut().item_spacing.y = theme.spacing_lg.value();
        card_slot(ui, theme, "default · hover the card to reveal ×", |ui| {
            let id = ui.id().with("g_banner_anatomy_card");
            let prev: Option<egui::Rect> = ui.data(|d| d.get_temp(id));
            let hovered = prev.is_some_and(|r| ui.rect_contains_pointer(r));
            let state = if hovered {
                MoreTriggerState::Hovered
            } else {
                MoreTriggerState::Hidden
            };
            let top = ui.cursor().min;
            banner_shell(ui, theme, 1.0, |ui| {
                mouse_capture_banner_body(ui, theme, state, None);
            });
            let rect = egui::Rect::from_min_max(top, ui.min_rect().max);
            ui.data_mut(|d| d.insert_temp(id, rect));
        });
        card_slot(ui, theme, "one-line body (short locale)", |ui| {
            banner_shell(ui, theme, 1.0, |ui| {
                mouse_capture_banner_body(
                    ui,
                    theme,
                    MoreTriggerState::Hidden,
                    Some("Mouse captured — hold Shift to select."),
                );
            });
        });
        card_slot(
            ui,
            theme,
            "recessed — dimmed behind a higher-scope banner",
            |ui| {
                // 시안은 카드 전체(셸 + 내용)에 opacity 를 건다. banner_shell 의 opacity 는 셸만 흐린다.
                ui.scope(|ui| {
                    ui.multiply_opacity(theme.opacity_recessed());
                    banner_shell(ui, theme, 1.0, |ui| {
                        mouse_capture_banner_body(ui, theme, MoreTriggerState::Hidden, None);
                    });
                });
            },
        );
    });

    spec::meta(
        ui,
        theme,
        &[
            ("title", "\"Mouse input captured\" · 13/600 banner-fg"),
            ("body", "caption · text-muted · 1–3 lines"),
            ("glyph", "mouse · banner-icon-fg"),
            ("affordance", "⋯ + × on hover (no TTL / countdown)"),
            ("fires", "once per tracking session · user click only"),
            ("dismiss", "suppresses for the session"),
        ],
        &[
            TokenChip::new("banner-bg", "card fill", theme.banner_bg().to_egui()),
            TokenChip::new(
                "banner-icon-fg",
                "mouse glyph",
                theme.banner_icon_fg().to_egui(),
            ),
            TokenChip::without_color("banner-recessed-opacity", "dimmed variant"),
        ],
    );
    spec::note(
        ui,
        theme,
        "The bypass keys (Shift+drag / Shift+Right-click) are the message — keep them in the \
         body. No inline action buttons: the body stays text-only, and the two per-app opt-outs \
         live behind the ⋯ trigger next to the × (see Banner more menu below).",
    );
}

pub fn draw_elastic(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Tight, |ui| {
        egui::Frame::new()
            .fill(theme.bg_app().to_egui())
            .corner_radius(theme.corner_radius.value())
            .inner_margin(egui::Margin::same(STAGE_PAD.value() as i8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing =
                        egui::vec2(ELASTIC_GAP.value(), ELASTIC_GAP.value());
                    elastic_slot(
                        ui,
                        theme,
                        "short name — content-sized (the vim row already passes the 200 floor)",
                        |ui| {
                            more_menu(ui, theme, "vim", None, false);
                        },
                    );
                    elastic_slot(ui, theme, "medium — grows to fit", |ui| {
                        more_menu(ui, theme, "python3.11", Some(0), false);
                    });
                    elastic_slot(
                        ui,
                        theme,
                        "long — capped at 288px, name ellipsises",
                        |ui| {
                            more_menu(ui, theme, "some-very-long-tool-name", None, false);
                        },
                    );
                    elastic_slot(
                        ui,
                        theme,
                        "rejected — danger tone on the capture row",
                        |ui| {
                            ui.multiply_opacity(REJECTED_OPACITY);
                            more_menu(ui, theme, "vim", None, true);
                        },
                    );
                });
            });
    });

    spec::meta(
        ui,
        theme,
        &[
            ("label", "fixed text + app name (2 spans)"),
            ("truncates", "the app name only"),
            ("app name", "mono · text-primary"),
            ("width", "content-sized, 200 ≤ w ≤ 288"),
            ("wrap", "never — 1 line per row"),
            ("tooltip", "full program name on the row"),
            ("tone", "both rows neutral (no danger)"),
        ],
        &[
            TokenChip::without_color("banner-more-app-font", "mono program name"),
            TokenChip::new(
                "banner-more-app-fg",
                "program name tone",
                theme.banner_more_app_fg().to_egui(),
            ),
            TokenChip::without_color("menu-item-height", "28px rows"),
            TokenChip::new(
                "accent-danger",
                "rejected variant only",
                theme.accent_danger().to_egui(),
            ),
        ],
    );
    spec::note(
        ui,
        theme,
        "The body menu (src/adapters/ui/mouse_capture_menu.rs) is a fixed 240px wide and its \
         first row reads \u{201c}Turn off this notification for\u{201d} \
         (popup.mouse_capture_banner_menu); this spec draws the kit elastic width and the kit \
         \u{201c}notice\u{201d} copy.",
    );
    spec::do_(
        ui,
        theme,
        "Do keep the program name as the emphasised, mono part of the row — it is the one thing \
         the user must verify before writing a permanent per-app setting, and mono marks it as a \
         process name rather than prose.",
    );
    spec::dont(
        ui,
        theme,
        "Don't paint the \u{201c}Disable mouse capture\u{201d} row in danger tone (last card above, \
         shown for the record). Nothing is destroyed and nothing is lost: the setting is \
         reversible from Settings › Terminal, and it returns the mouse to tasty. Danger in this \
         system means delete — spending it here would make the two rows look like different \
         classes of action when they are the same class at two strengths.",
    );
}

/// 라벨 + 메뉴 한 칸. 줄바꿈 줄에서 칸째로 넘어가도록 wrap_item 으로 감싼다.
fn elastic_slot(ui: &mut egui::Ui, theme: &Theme, label: &str, add: impl FnOnce(&mut egui::Ui)) {
    spec::wrap_item(ui, |ui| {
        ui.spacing_mut().item_spacing.y = ELASTIC_LABEL_GAP.value();
        caption_label(ui, theme, label);
        add(ui);
    });
}

/// 두 행 메뉴 — 폭은 내용에 맞추되 min/max-width 토큰 사이로 묶는다.
fn more_menu(ui: &mut egui::Ui, theme: &Theme, app: &str, hovered: Option<usize>, danger: bool) {
    let pad = theme.banner_more_menu_padding().value();
    let bw = theme.border_width.value();
    let chrome = (pad + bw) * 2.0;
    let body = theme.font_size_body.value();
    let text_w = |text: &str, font: egui::FontId| {
        ui.fonts(|f| {
            f.layout_no_wrap(text.to_owned(), font, egui::Color32::PLACEHOLDER)
                .size()
                .x
        })
    };
    let app_w = text_w(app, egui::FontId::monospace(body));
    let prefix_w = text_w(SUPPRESS_PREFIX, egui::FontId::proportional(body))
        .max(text_w(DISABLE_PREFIX, egui::FontId::proportional(body)));
    let row_w = theme.menu_item_padding_x().value() * 2.0
        + theme.icon_glyph_size_md.value()
        + theme.spacing_sm.value()
        + prefix_w
        + app_w;
    let outer = (row_w + chrome).clamp(
        theme.banner_more_menu_min_width().value(),
        theme.banner_more_menu_max_width().value(),
    );
    let resp = egui::Frame::new()
        .fill(theme.banner_more_menu_bg().to_egui())
        .stroke(egui::Stroke::new(
            bw,
            theme.banner_more_menu_border().to_egui(),
        ))
        .corner_radius(theme.banner_more_menu_radius().value())
        .shadow(theme.shadow_popover().to_egui())
        .inner_margin(egui::Margin::same(pad as i8))
        .show(ui, |ui| {
            ui.set_width(outer - chrome);
            ui.spacing_mut().item_spacing.y = 0.0;
            let rows = [
                (icons::BELL, SUPPRESS_PREFIX, false),
                (icons::MOUSE, DISABLE_PREFIX, danger),
            ];
            for (i, (icon, prefix, tone)) in rows.into_iter().enumerate() {
                // 시안 MenuItem `active` 는 surface-active 채움이다. 행 헬퍼의 hover 채움
                // (overlay-hover)은 쓰지 않고, 행 아래 자리를 먼저 잡아 두었다가 채운다.
                let fill = ui.painter().add(egui::Shape::Noop);
                let top = ui.cursor().min;
                let w = ui.available_width();
                mouse_capture_menu_row_tone(ui, theme, icon, prefix, app, "", false, tone);
                if hovered == Some(i) {
                    let rect = egui::Rect::from_min_size(
                        top,
                        egui::vec2(w, theme.menu_item_height().value()),
                    );
                    ui.painter().set(
                        fill,
                        egui::Shape::rect_filled(
                            rect,
                            theme.menu_item_radius().value(),
                            theme.surface_active().to_egui(),
                        ),
                    );
                }
            }
        });
    resp.response.on_hover_text(app);
}
