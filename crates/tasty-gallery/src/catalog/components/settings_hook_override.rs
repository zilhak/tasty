//! Hook Handlers — 사용자 patch 가 걸린 host·plugin 행의 "edited" 표시와 Revert, 되돌리기 대기 상태.
//! 시안 `HookOverrideG` 를 옮긴 예제다. 행 모양은 본체 `file_handler_tab/hook_handlers.rs` 와 같다.

use std::cell::RefCell;

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{Button, ButtonVariant, ControlSize, TagVariant, switch, tag, tag_disabled};

use crate::catalog::icons;
use crate::catalog::spec::{self, StageVariant, TokenChip};

/// 예제 행. `default` 는 되돌리기 대기 중에 보이는 기본 요약이다.
struct Row {
    event: &'static str,
    action: &'static str,
    default: &'static str,
    origin: &'static str,
    edited: bool,
    on: bool,
    pending: bool,
}

/// 시안의 네 행 — host 편집, plugin 끔, host 되돌리기 대기, 편집 없는 host.
fn seed() -> Vec<Row> {
    vec![
        Row {
            event: "on_open",
            action: "ipc: focus → open_markdown_preview",
            default: "open_markdown_preview",
            origin: "host",
            edited: true,
            on: true,
            pending: false,
        },
        Row {
            event: "on_paste",
            action: "imgview.stash",
            default: "imgview.stash",
            origin: "dev.imgview",
            edited: true,
            on: false,
            pending: false,
        },
        Row {
            event: "on_open",
            action: "ipc: focus → open_markdown_preview",
            default: "open_markdown_preview",
            origin: "host",
            edited: true,
            on: true,
            pending: true,
        },
        Row {
            event: "on_exit",
            action: "ipc: focus → save → close",
            default: "ipc: focus → save → close",
            origin: "host",
            edited: false,
            on: true,
            pending: false,
        },
    ]
}

thread_local! {
    /// 테마 짝 두 벌의 상태. Revert ↔ Undo 를 눌러 볼 수 있다.
    static ROWS: RefCell<[Vec<Row>; 2]> = RefCell::new([seed(), seed()]);
}

/// 시안 `ThemePair` — Mocha·Latte 를 `bg-app` 바탕에 위아래로 그린다.
pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    let latte = crate::host_shell::latte_theme();
    let mocha = tasty_themes::mocha_fallback();
    spec::stage(ui, theme, StageVariant::Solo, |ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = theme.spacing_lg.value();
            ROWS.with(|r| {
                let sets = &mut *r.borrow_mut();
                for (th, rows) in [&mocha, &latte].into_iter().zip(sets.iter_mut()) {
                    egui::Frame::new()
                        .fill(th.bg_app().to_egui())
                        .inner_margin(egui::Margin::same(theme.spacing_lg.value() as i8))
                        .show(ui, |ui| table(ui, th, rows));
                }
            });
        });
    });
    spec::meta(
        ui,
        theme,
        &[
            (
                "mark",
                "Tag (neutral) \"edited\" after the origin Tag — host / plugin rows with a user patch only",
            ),
            (
                "mark tooltip",
                "Changed in your settings. Updates to the default no longer apply.",
            ),
            (
                "action",
                "Revert · ghost Button sm · action line, left of Edit",
            ),
            ("Revert tooltip", "Go back to the default from {origin}."),
            (
                "pending",
                "button → Undo (same slot) · Tag \"reverts on save\" (disabled) · summary shows the default",
            ),
            (
                "scope",
                "sequence edit · Switch off · both — any user patch",
            ),
            (
                "Save / Cancel",
                "Save removes the patch · Cancel restores it",
            ),
            ("user rows", "never marked — they are the user's own"),
        ],
        &[
            TokenChip::new(
                "tag-disabled-bg",
                "pending Tag",
                theme.tag_disabled_bg().to_egui(),
            ),
            TokenChip::new("glyph-dim", "padlock", theme.glyph_dim().to_egui()),
            TokenChip::new(
                "accent-agent",
                "plugin origin",
                theme.accent_agent().to_egui(),
            ),
        ],
    );
    spec::note(
        ui,
        theme,
        "No new tokens: the mark is the shared Tag, the pending state reuses the extension-mapping Undo + disabled Tag pair.",
    );
}

/// 행 목록 — 560 폭 패널, 강한 테두리.
fn table(ui: &mut egui::Ui, th: &Theme, rows: &mut [Row]) {
    egui::Frame::new()
        .fill(th.bg_panel().to_egui())
        .stroke(egui::Stroke::new(
            th.border_width.value(),
            th.border_strong().to_egui(),
        ))
        .corner_radius(th.corner_radius.value())
        .show(ui, |ui| {
            ui.set_width(th.measure_xl.value());
            ui.spacing_mut().item_spacing.y = 0.0;
            for row in rows.iter_mut() {
                draw_row(ui, th, row);
            }
        });
}

fn draw_row(ui: &mut egui::Ui, th: &Theme, r: &mut Row) {
    let plugin = r.origin != "host";
    let resp = egui::Frame::NONE
        .inner_margin(egui::Margin::symmetric(
            th.spacing_md.value() as i8,
            th.spacing_sm.value() as i8,
        ))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = th.spacing_xs.value();
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = th.spacing_sm.value();
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let side = ControlSize::Sm.icon_glyph(th);
                    let (rect, lock) =
                        ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::hover());
                    icons::LOCK
                        .image(side, th.glyph_dim().into())
                        .paint_at(ui, rect);
                    lock.on_hover_text(format!("Provided by {} — can't be removed", r.origin));
                    let mut shown_on = if r.pending { true } else { r.on };
                    if switch(ui, th, &mut shown_on, None, true).changed() && !r.pending {
                        r.on = shown_on;
                    }
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = th.spacing_sm.value();
                        ui.label(
                            egui::RichText::new(r.event)
                                .monospace()
                                .size(th.font_size_caption.value())
                                .color(th.text_secondary().to_egui()),
                        );
                        let variant = if plugin {
                            TagVariant::Agent
                        } else {
                            TagVariant::Default
                        };
                        tag(ui, th, r.origin, variant, false);
                        if r.edited && r.pending {
                            tag_disabled(ui, th, "reverts on save", false);
                        } else if r.edited {
                            tag(ui, th, "edited", TagVariant::Default, false).on_hover_text(
                                "Changed in your settings. Updates to the default no longer apply.",
                            );
                        }
                    });
                });
            });
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = th.spacing_sm.value();
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let summary = if r.pending { r.default } else { r.action };
                    if summary.starts_with("ipc:") {
                        Button::new("Edit")
                            .variant(ButtonVariant::Ghost)
                            .size(ControlSize::Sm)
                            .show(ui, th);
                    }
                    if r.edited {
                        let label = if r.pending { "Undo" } else { "Revert" };
                        let resp = Button::new(label)
                            .variant(ButtonVariant::Ghost)
                            .size(ControlSize::Sm)
                            .show(ui, th);
                        let resp = if r.pending {
                            resp
                        } else {
                            resp.on_hover_text(format!("Go back to the default from {}.", r.origin))
                        };
                        if resp.clicked() {
                            r.pending = !r.pending;
                        }
                    }
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(summary)
                                    .monospace()
                                    .size(th.font_size_caption.value())
                                    .color(th.text_primary().to_egui()),
                            )
                            .truncate(),
                        );
                    });
                });
            });
        });
    let rect = resp.response.rect;
    ui.painter().hline(
        rect.x_range(),
        rect.bottom(),
        egui::Stroke::new(
            th.border_width.value(),
            th.separator.to_egui_premultiplied(),
        ),
    );
}
