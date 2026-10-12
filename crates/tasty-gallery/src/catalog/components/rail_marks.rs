//! 접힌 레일의 표지 예제. mirror 칩을 Mocha·Latte 두 테마에서 상태별로 비교한다.
//! 치수와 토큰은 본체 접힌 레일의 워크스페이스 아바타와 같다.

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{RailDot, paint_rail_dot};

use crate::catalog::spec::{self, StageVariant, TokenChip};

/// 예제 아바타의 고정 상태.
#[derive(Clone, Copy, Default)]
struct Avatar {
    letter: &'static str,
    active: bool,
    hover: bool,
    notif: bool,
    ring: bool,
    mirror: bool,
}

/// 아바타 크기. 본체 `collapsed_ws_size`와 같다.
fn avatar_size(t: &Theme) -> egui::Vec2 {
    egui::vec2(
        t.sidebar_collapsed_slot_width.value(),
        t.sidebar_collapsed_workspace_height.value(),
    )
}

fn paint_avatar(ui: &mut egui::Ui, t: &Theme, rect: egui::Rect, a: Avatar) {
    let radius = t.corner_radius.value();
    if a.active {
        ui.painter()
            .rect_filled(rect, radius, t.overlay_active().to_egui_premultiplied());
    }
    if a.hover {
        ui.painter()
            .rect_filled(rect, radius, t.hover_overlay.to_egui_premultiplied());
    }
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        a.letter,
        egui::FontId::monospace(t.font_size_body.value()),
        egui::Color32::from(if a.active {
            t.accent_primary()
        } else {
            t.text_muted()
        }),
    );
    if a.notif {
        paint_rail_dot(ui.painter(), t, rect, RailDot::Completion);
    }
    if a.ring {
        ui.painter().rect_stroke(
            rect,
            radius,
            egui::Stroke::new(
                t.status_dot_attached_ring_width().value(),
                egui::Color32::from(t.border_attached()),
            ),
            egui::StrokeKind::Inside,
        );
    }
    if a.mirror {
        super::sidebar::mirror_corner_chip(ui, t, rect);
    }
}

/// 표본 한 칸. 위에 표지, 아래에 상태 이름을 둔다.
fn cell(
    ui: &mut egui::Ui,
    t: &Theme,
    caption: &str,
    paint: impl FnOnce(&mut egui::Ui, egui::Rect),
) {
    let caption = egui::RichText::new(caption)
        .monospace()
        .size(t.font_size_micro.value())
        .color(t.text_muted().to_egui());
    // 칸은 최소 64이고, 상태 이름이 더 길면 이름 폭을 따른다.
    let caption_w = egui::WidgetText::from(caption.clone())
        .into_galley(
            ui,
            Some(egui::TextWrapMode::Extend),
            f32::INFINITY,
            egui::TextStyle::Body,
        )
        .size()
        .x;
    let col_w = (t.spacing_xl.value() * 2.0 + t.spacing_lg.value()).max(caption_w);
    let mark_h = t.spacing_xl.value() + t.spacing_md.value();
    ui.allocate_ui_with_layout(
        egui::vec2(col_w, 0.0),
        egui::Layout::top_down(egui::Align::Center),
        |ui| {
            ui.spacing_mut().item_spacing.y = t.spacing_sm.value();
            let (slot, _) = ui.allocate_exact_size(egui::vec2(col_w, mark_h), egui::Sense::hover());
            paint(ui, slot);
            ui.label(caption);
        },
    );
}

fn heading(ui: &mut egui::Ui, t: &Theme, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .size(t.font_size_caption.value())
            .color(t.text_secondary().to_egui()),
    );
}

fn centered(slot: egui::Rect, size: egui::Vec2) -> egui::Rect {
    egui::Rect::from_center_size(slot.center(), size)
}

fn board(ui: &mut egui::Ui, t: &Theme, name: &str) {
    egui::Frame::new()
        .fill(t.bg_sidebar().to_egui())
        .stroke(egui::Stroke::new(
            t.border_width.value(),
            t.border_default().to_egui(),
        ))
        .corner_radius(t.corner_radius.value())
        .inner_margin(egui::Margin::same(t.spacing_md.value() as i8))
        .show(ui, |ui| {
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = t.spacing_lg.value();
                    heading(ui, t, &format!("{name} — mirror chip"));
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = t.spacing_md.value();
                        let base = Avatar {
                            letter: "E",
                            mirror: true,
                            ..Avatar::default()
                        };
                        for (caption, a) in [
                            ("idle", base),
                            (
                                "active",
                                Avatar {
                                    letter: "T",
                                    active: true,
                                    ..base
                                },
                            ),
                            (
                                "hover",
                                Avatar {
                                    hover: true,
                                    ..base
                                },
                            ),
                            (
                                "+ notif",
                                Avatar {
                                    notif: true,
                                    ..base
                                },
                            ),
                            ("+ attached ring", Avatar { ring: true, ..base }),
                        ] {
                            cell(ui, t, caption, |ui, slot| {
                                paint_avatar(ui, t, centered(slot, avatar_size(t)), a)
                            });
                        }
                    });
                });
            });
        });
}

/// 시안 "Rail marks — category line and mirror chip"의 mirror 칩 보드를 Mocha·Latte로 그린다.
pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    let latte = crate::host_shell::latte_theme();
    let mocha = tasty_themes::mocha_fallback();
    spec::stage(ui, theme, StageVariant::Tight, |ui| {
        egui::Frame::new()
            .fill(theme.bg_app().to_egui())
            .inner_margin(egui::Margin::same(theme.spacing_lg.value() as i8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = theme.spacing_lg.value();
                    board(ui, &mocha, "Mocha");
                    board(ui, &latte, "Latte");
                });
            });
    });
    spec::meta(
        ui,
        theme,
        &[
            (
                "chip",
                "12 circle · 1px edge · 8 glyph · 2px bg-sidebar halo · bottom-right",
            ),
            (
                "chip edge",
                "workspace-mirror-chip-border → workspace-mirror-fg",
            ),
            (
                "avatar",
                "32 × 28 rail slot (the design board draws 36 × 36)",
            ),
        ],
        &[
            TokenChip::new(
                "workspace-mirror-chip-border",
                "chip edge",
                theme.workspace_mirror_chip_border().into(),
            ),
            TokenChip::new(
                "workspace-mirror-chip-bg",
                "chip bed",
                theme.workspace_mirror_chip_bg().into(),
            ),
        ],
    );
}
