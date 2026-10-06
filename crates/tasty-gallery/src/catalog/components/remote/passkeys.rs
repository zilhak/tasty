//! 원격 도구 Passkeys 탭 예제 — 시안 `PasskeyRow` 의 값 가림·보임·모르는 kind 상태를 Mocha·Latte 로 그린다.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::tokens::{STRUCT_GAP_1, STRUCT_GAP_2};
use tasty_ui_widgets::{Button, ButtonVariant, IconButton, IconButtonVariant, TagVariant, tag};

use super::{WARN_BADGE_HEIGHT, WIDTH, attach_header, tab_bar};
use crate::catalog::icons;
use crate::catalog::spec::{self, StageVariant, TokenChip};
use crate::catalog::widgets::dialog as kit;

struct PasskeySample {
    name: &'static str,
    kind: &'static str,
    value: &'static str,
    revealed: bool,
}

/// 시안 `RemoteFrame tab="passkeys"` 의 네 행 — 가림, 보임(active + eyeOff, 긴 경로는
/// 끝 말줄임), 가림, 모르는 kind. 시안 seed 의 kind `file`·`secret` 은 tasty 의 kind 인
/// `path`·`inline` 으로 옮긴다.
const PASSKEYS: &[PasskeySample] = &[
    PasskeySample {
        name: "ed25519-main",
        kind: "path",
        value: "~/.ssh/id_ed25519",
        revealed: false,
    },
    PasskeySample {
        name: "deploy-key-with-a-very-long-name",
        kind: "path",
        value: "/Users/hyunjun/Library/Application Support/tasty/keys/deploy/ci-runner/id_ed25519_deploy",
        revealed: true,
    },
    PasskeySample {
        name: "vault-token",
        kind: "inline",
        value: "",
        revealed: false,
    },
    PasskeySample {
        name: "legacy",
        kind: "agent-x",
        value: "",
        revealed: false,
    },
];

/// 시안 `KIND_OPTIONS`. 이 밖의 kind 는 Tag 대신 경고 배지로 보인다.
const KNOWN_PASSKEY_KINDS: &[&str] = &["path", "inline"];

/// 값을 가리는 자리표시. 본체와 같은 마스크다.
const PASSKEY_MASK: &str = "••••••••";

pub fn draw_passkeys(ui: &mut egui::Ui, theme: &Theme) {
    let with_zoom =
        |base: Theme| Theme::with_colors_and_zoom(base.to_colors(), base.is_light, theme.ui_zoom);
    let themes = [
        with_zoom(tasty_themes::mocha_fallback()),
        with_zoom(crate::host_shell::latte_theme()),
    ];
    spec::stage(ui, theme, StageVariant::Column, |ui| {
        for (i, th) in themes.iter().enumerate() {
            ui.push_id(i, |ui| passkeys_frame(ui, th));
        }
    });

    spec::meta(
        ui,
        theme,
        &[
            (
                "row",
                "pad space-md space-xs · name 13 · kind Tag (unknown kind → warn badge)",
            ),
            (
                "value",
                "kind · value-or-mask · mono 11 · one line, ellipsis revealed or not",
            ),
            ("mask", "fixed 8-dot mask while hidden"),
            (
                "long value",
                "ellipsis at the end · actions never pushed out of the row",
            ),
            (
                "actions",
                "reveal · edit · trash — IconButton sm, gap size-1",
            ),
            (
                "revealed",
                "IconButton active (accent glyph + overlay-active) and eye → eyeOff",
            ),
            ("add-bar", "Add passkey — secondary sm + plus"),
        ],
        &[
            TokenChip::new(
                "accent-primary",
                "revealed glyph",
                theme.accent_primary().to_egui(),
            ),
            TokenChip::without_color("overlay-active", "revealed button fill"),
            TokenChip::new("text-muted", "value mono", theme.text_muted().to_egui()),
            TokenChip::without_color("separator", "row dividers"),
        ],
    );
}

/// 시안 `RemoteFrame tab="passkeys"` — 머리 · 탭 · add-bar · 목록.
fn passkeys_frame(ui: &mut egui::Ui, theme: &Theme) {
    kit::frame_card(ui, theme, WIDTH, kit::panel_fill(theme), |ui| {
        attach_header(ui, theme);
        tab_bar(ui, theme, 2);

        kit::region_sym(ui, theme.spacing_md, theme.spacing_sm, |ui| {
            ui.horizontal(|ui| {
                Button::new("Add passkey")
                    .variant(ButtonVariant::Secondary)
                    .size(tasty_ui_widgets::ControlSize::Sm)
                    .leading_icon(&|ui, rect, c| {
                        icons::PLUS.image(rect.height(), c).paint_at(ui, rect)
                    })
                    .show(ui, theme);
            });
        });

        kit::region_sym(ui, theme.spacing_md, LogicalPx(0.0), |ui| {
            for k in PASSKEYS {
                passkey_row(ui, theme, k);
            }
        });
    });
}

fn passkey_row(ui: &mut egui::Ui, theme: &Theme, k: &PasskeySample) {
    kit::region_sym(ui, theme.spacing_xs, theme.spacing_md, |ui| {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
            // 동작 묶음의 폭을 먼저 빼서 긴 값이 버튼을 밀어내지 않게 한다.
            let button = tasty_ui_widgets::ControlSize::Sm.height(theme);
            let gap = STRUCT_GAP_1.value();
            let actions_w = button * 3.0 + gap * 2.0;
            let text_w = (ui.available_width() - actions_w - theme.spacing_sm.value()).max(0.0);
            ui.allocate_ui_with_layout(
                egui::vec2(text_w, 0.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_width(text_w);
                    ui.spacing_mut().item_spacing.y = STRUCT_GAP_2.value();
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(k.name)
                                    .size(theme.font_size_body.value())
                                    .strong()
                                    .color(theme.text_primary().to_egui()),
                            )
                            .truncate(),
                        );
                        if KNOWN_PASSKEY_KINDS.contains(&k.kind) {
                            tag(ui, theme, k.kind, TagVariant::Default, false);
                        } else {
                            warn_badge(ui, theme, k.kind);
                        }
                    });
                    let value = if k.revealed { k.value } else { PASSKEY_MASK };
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(format!("{} · {}", k.kind, value))
                                .monospace()
                                .size(theme.font_size_caption.value())
                                .color(theme.text_muted().to_egui()),
                        )
                        .truncate(),
                    );
                },
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for glyph in [icons::TRASH, icons::EDIT] {
                    IconButton::new()
                        .variant(IconButtonVariant::Ghost)
                        .size(tasty_ui_widgets::ControlSize::Sm)
                        .show(ui, theme, &|ui, rect, c| {
                            glyph.image(rect.height(), c).paint_at(ui, rect)
                        });
                }
                let reveal = if k.revealed {
                    icons::EYE_OFF
                } else {
                    icons::EYE
                };
                IconButton::new()
                    .variant(IconButtonVariant::Ghost)
                    .size(tasty_ui_widgets::ControlSize::Sm)
                    .active(k.revealed)
                    .show(ui, theme, &|ui, rect, c| {
                        reveal.image(rect.height(), c).paint_at(ui, rect)
                    });
            });
        });
    });
    kit::hsep(ui, theme);
}

/// 시안 `WarnBadge` — 경고 아이콘(12) + mono micro 텍스트, 좌우 space-sm.
/// 채움·테두리·높이는 [`super::warn_pill`] 과 같은 배지 규칙이다.
fn warn_badge(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    let warn = theme.accent_warning().to_egui();
    let galley = ui.painter().layout_no_wrap(
        text.to_owned(),
        egui::FontId::monospace(theme.font_size_micro.value()),
        egui::Color32::PLACEHOLDER,
    );
    let pad_x = theme.spacing_sm.value();
    let glyph = theme.icon_glyph_size_xs.value();
    let gap = theme.spacing_xs.value();
    let h = WARN_BADGE_HEIGHT.value();
    let w = pad_x * 2.0 + glyph + gap + galley.rect.width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
    let radius = theme.corner_radius_sm.value();
    const BADGE_STROKE_OPACITY: f32 = 0.4;
    ui.painter()
        .rect_filled(rect, radius, warn.gamma_multiply(theme.tint_fill_alpha()));
    ui.painter().rect_stroke(
        rect,
        radius,
        egui::Stroke::new(
            theme.border_width.value(),
            warn.gamma_multiply(BADGE_STROKE_OPACITY),
        ),
        egui::StrokeKind::Inside,
    );
    let glyph_rect = egui::Rect::from_min_size(
        egui::pos2(rect.left() + pad_x, rect.center().y - glyph * 0.5),
        egui::vec2(glyph, glyph),
    );
    icons::ALERT_TRIANGLE
        .image(glyph, warn)
        .paint_at(ui, glyph_rect);
    let pos = egui::pos2(
        glyph_rect.right() + gap,
        rect.center().y - galley.rect.height() * 0.5,
    );
    ui.painter().galley(pos, galley, warn);
}
