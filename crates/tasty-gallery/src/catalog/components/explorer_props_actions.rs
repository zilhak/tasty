//! 탐색기 Properties 의 동작 예제: 시각 형식별 시각 필드 · Open footer · 권한 편집 상자.
//! 시안 `gallery/explorer-ops-props-actions.jsx` 의 "Properties — time format · Open · permissions" 구역을 옮긴다.
//! 본체는 `explorer_properties` popup 과 그 `perm_edit` 하위 모듈이 같은 배치를 그린다.

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{Button, ButtonVariant, ControlSize, Spinner};

use super::explorer_file_info::{Extras, field, mono, props_card_full, time, warn};
use crate::catalog::icons::{self, MockGlyph};
use crate::catalog::spec::{StageVariant, TokenChip, cluster, meta, note, stage};
use crate::i18n::t;

/// 시안 `TF` 의 예시 시각.
const TIMES: [(&str, &str); 3] = [
    ("minutes", "2026-10-21 12:12"),
    ("seconds", "2026-10-21 12:12:05"),
    ("seconds_offset", "2026-10-21 12:12:05 +09:00"),
];

/// 편집 상자 예제의 상태.
#[derive(Clone, Copy, PartialEq, Eq)]
enum BoxState {
    Editing,
    Applying,
    Failed(&'static str),
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    times(ui, theme);
    footer(ui, theme);
    permissions(ui, theme);
}

fn times(ui: &mut egui::Ui, theme: &Theme) {
    stage(ui, theme, StageVariant::Wrap, |ui| {
        for (setting, sample) in TIMES {
            cluster(ui, theme, setting, |ui| {
                card(
                    ui,
                    theme,
                    (icons::IMAGE, "diagram.png"),
                    &[
                        time("explorer.properties.modified", sample),
                        time(
                            "explorer.properties.created",
                            &sample.replacen("12:12", "11:58", 1),
                        ),
                    ],
                    None,
                    Extras {
                        footer: Some(true),
                        ..Extras::default()
                    },
                );
            });
        }
    });
    meta(
        ui,
        theme,
        &[
            (
                "setting",
                "[general] explorer_props_time_format · minutes (default) · seconds · seconds_offset · local time zone · numeric offset",
            ),
            (
                "wrap",
                "mono caption · wraps only between date · time · offset when the cell is narrower than the popup",
            ),
            (
                "applies",
                "open Properties follow the setting on the next draw",
            ),
        ],
        &[],
    );
}

fn footer(ui: &mut egui::Ui, theme: &Theme) {
    let broken = t("explorer.props.target_missing");
    stage(ui, theme, StageVariant::Wrap, |ui| {
        cluster(ui, theme, "file — Open as Enter", |ui| {
            card(
                ui,
                theme,
                (icons::IMAGE, "diagram.png"),
                &[
                    field("explorer.properties.kind", "PNG image"),
                    time("explorer.properties.modified", TIMES[0].1),
                    mono("explorer.props.path", "~/Downloads/diagram.png", true),
                    editable("rw-r--r-- · read-only: no"),
                ],
                None,
                Extras {
                    footer: Some(true),
                    ..Extras::default()
                },
            );
        });
        cluster(ui, theme, "broken link — Open disabled", |ui| {
            card(
                ui,
                theme,
                (icons::LINK, "old-config.toml"),
                &[
                    field("explorer.properties.kind", t("explorer.kind.broken_link")),
                    warn(
                        mono(
                            "explorer.properties.link_target",
                            "~/.config/tasty/old-config.toml",
                            true,
                        ),
                        "explorer.props.target_missing",
                    ),
                ],
                Some(t("explorer.props.link_note")),
                Extras {
                    footer: Some(false),
                    ..Extras::default()
                },
            );
        });
        cluster(ui, theme, "several items — no footer", |ui| {
            card(
                ui,
                theme,
                (icons::LAYERS, "3 items"),
                &[
                    field("explorer.properties.kinds", "2 files, 1 folder"),
                    mono("explorer.properties.location", "~/Downloads", true),
                ],
                None,
                Extras::default(),
            );
        });
    });
    meta(
        ui,
        theme,
        &[
            (
                "footer",
                "padding space-sm explorer-props-padding-x · top 1px explorer-props-footer-border · justify end",
            ),
            (
                "button",
                "secondary sm Open · same action as Enter for the popup's own item · popup stays open",
            ),
            (
                "shown",
                "one item: file · folder · link (target) · remote · broken link disabled with a tooltip · none for several items, the open folder itself or an unreadable item",
            ),
        ],
        &[TokenChip::new(
            "explorer-props-footer-border",
            "footer rule",
            theme.explorer_props_footer_border().to_egui_premultiplied(),
        )],
    );
    note(
        ui,
        theme,
        &format!("The disabled Open shows the tooltip “{broken}”."),
    );
}

fn permissions(ui: &mut egui::Ui, theme: &Theme) {
    stage(ui, theme, StageVariant::Wrap, |ui| {
        let unix = |ui: &mut egui::Ui,
                    label: &str,
                    glyph: (MockGlyph, &str),
                    bits: u32,
                    folder: bool,
                    special: bool,
                    state: BoxState| {
            let edit =
                move |ui: &mut egui::Ui| perm_box(ui, theme, Some(bits), folder, special, state);
            cluster(ui, theme, label, |ui| {
                card(
                    ui,
                    theme,
                    glyph,
                    &[
                        field(
                            "explorer.properties.kind",
                            t(if folder {
                                "explorer.type.folder"
                            } else {
                                "explorer.type.file"
                            }),
                        ),
                        mono(
                            "explorer.properties.permissions",
                            &format!("{} · read-only: no", rwx(bits)),
                            false,
                        ),
                    ],
                    None,
                    Extras {
                        under: Some(&edit),
                        footer: Some(true),
                    },
                );
            });
        };
        unix(
            ui,
            "editing",
            (icons::FILE, "deploy.env"),
            0o644,
            false,
            false,
            BoxState::Editing,
        );
        unix(
            ui,
            "folder — applying",
            (icons::FOLDER, "shared"),
            0o775,
            true,
            true,
            BoxState::Applying,
        );
        unix(
            ui,
            "failed — real mode re-read",
            (icons::FILE, "mounted.iso"),
            0o644,
            false,
            false,
            BoxState::Failed("Read-only file system (os error 30)"),
        );
        cluster(ui, theme, "not the owner — no Edit", |ui| {
            card(
                ui,
                theme,
                (icons::FILE, "hosts"),
                &[
                    mono("explorer.props.path", "/etc/hosts", true),
                    super::explorer_file_info::Field {
                        locked: true,
                        ..mono(
                            "explorer.properties.permissions",
                            "rw-r--r-- · read-only: no",
                            false,
                        )
                    },
                ],
                None,
                Extras {
                    footer: Some(true),
                    ..Extras::default()
                },
            );
        });
        for (label, state) in [
            ("Windows — read-only attribute", BoxState::Editing),
            (
                "Windows — failed",
                BoxState::Failed("Access is denied. (os error 5)"),
            ),
        ] {
            let edit = move |ui: &mut egui::Ui| perm_box(ui, theme, None, false, false, state);
            cluster(ui, theme, label, |ui| {
                card(
                    ui,
                    theme,
                    (icons::FILE, "report.docx"),
                    &[mono(
                        "explorer.properties.permissions",
                        "read-only: no",
                        false,
                    )],
                    None,
                    Extras {
                        under: Some(&edit),
                        footer: Some(true),
                    },
                );
            });
        }
    });
    meta(
        ui,
        theme,
        &[
            (
                "entry",
                "Edit ghost sm at the end of the Permissions value · hidden while the box is open",
            ),
            (
                "box",
                "under the row, full content width (the 3 × 56 grid is wider than the value column) · 1px explorer-props-perm-border · radius · padding space-sm · gap space-sm",
            ),
            (
                "grid",
                "role column + Read · Write · Execute · cell explorer-props-perm-cell-width · rows row-min-height",
            ),
            (
                "apply",
                "primary sm, off until changed · applying: spinner + Applying…, every control off · success closes and re-reads · failure keeps the box, shows the OS reason and re-reads the real mode",
            ),
            (
                "scope",
                "one local item that is not a link · Unix nine rwx bits, special bits kept, owner or root · Windows read-only attribute only · never recursive · no elevation · Esc never undoes",
            ),
        ],
        &[
            TokenChip::new(
                "explorer-props-perm-border",
                "box border",
                theme.explorer_props_perm_border().to_egui(),
            ),
            TokenChip::without_color("explorer-props-perm-cell-width", "grid cell"),
            TokenChip::new(
                "explorer-props-perm-error-fg",
                "failure line",
                theme.explorer_props_perm_error_fg().to_egui(),
            ),
            TokenChip::new(
                "explorer-props-perm-locked-fg",
                "not the owner",
                theme.explorer_props_perm_locked_fg().to_egui(),
            ),
        ],
    );
}

fn card(
    ui: &mut egui::Ui,
    theme: &Theme,
    head: (MockGlyph, &str),
    fields: &[super::explorer_file_info::Field],
    note_text: Option<&str>,
    extras: Extras<'_>,
) {
    props_card_full(ui, theme, head, fields, note_text, None, extras);
}

fn editable(value: &str) -> super::explorer_file_info::Field {
    super::explorer_file_info::Field {
        edit: true,
        ..mono("explorer.properties.permissions", value, false)
    }
}

/// 본체 `perm_edit::readout` 과 같은 글자.
fn rwx(bits: u32) -> String {
    [6u32, 3, 0]
        .iter()
        .map(|shift| {
            let triad = (bits >> shift) & 0o7;
            format!(
                "{}{}{}",
                if triad & 4 != 0 { 'r' } else { '-' },
                if triad & 2 != 0 { 'w' } else { '-' },
                if triad & 1 != 0 { 'x' } else { '-' }
            )
        })
        .collect()
}

/// 본체 `perm_edit::draw` 의 배치. `bits` 가 None 이면 Windows 읽기 전용 상자다.
/// 편집 예제는 기타의 비트를 끈 상태(바뀜)로, 실패 예제는 다시 읽은 원래 값으로 보인다.
fn perm_box(
    ui: &mut egui::Ui,
    theme: &Theme,
    bits: Option<u32>,
    folder: bool,
    special: bool,
    state: BoxState,
) {
    let busy = state == BoxState::Applying;
    let failed = matches!(state, BoxState::Failed(_));
    let gap = theme.spacing_sm.value();
    ui.add_space(theme.spacing_xs.value());
    egui::Frame::new()
        .stroke(egui::Stroke::new(
            theme.border_width.value(),
            theme.explorer_props_perm_border().to_egui(),
        ))
        .corner_radius(theme.corner_radius.value())
        .inner_margin(gap as i8)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing = egui::vec2(gap, gap);
            let caption = theme.font_size_caption.value();
            match bits {
                Some(from) => {
                    let shown = if failed { from } else { from & !0o7 };
                    grid(ui, theme, shown, busy);
                    ui.label(
                        egui::RichText::new(format!("{} · {:03o}", rwx(shown), shown))
                            .monospace()
                            .size(caption)
                            .color(theme.text_secondary().to_egui()),
                    );
                    let mut lines = Vec::new();
                    if folder {
                        lines.push(t("explorer.props.perm.folder_only"));
                    }
                    if special {
                        lines.push(t("explorer.props.perm.special_kept"));
                    }
                    if !lines.is_empty() {
                        muted(ui, theme, &lines.join(" "));
                    }
                }
                None => {
                    let mut on = !failed;
                    tasty_ui_widgets::checkbox(
                        ui,
                        theme,
                        &mut on,
                        t("explorer.props.perm.read_only"),
                        !busy,
                    );
                    muted(ui, theme, t("explorer.props.perm.windows_hint"));
                }
            }
            if let BoxState::Failed(reason) = state {
                let error = theme.explorer_props_perm_error_fg().to_egui();
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = theme.spacing_xs.value();
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = theme.spacing_xs.value();
                        let g = theme.icon_glyph_size_sm.value();
                        let (r, _) = ui.allocate_exact_size(egui::vec2(g, g), egui::Sense::hover());
                        icons::ALERT_CIRCLE.image(g, error).paint_at(ui, r);
                        ui.label(
                            egui::RichText::new(t("explorer.props.perm.failed"))
                                .size(caption)
                                .color(error),
                        );
                    });
                    ui.label(
                        egui::RichText::new(reason)
                            .monospace()
                            .size(caption)
                            .color(theme.text_muted().to_egui()),
                    );
                });
            }
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), ControlSize::Sm.height(theme)),
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    let size = theme.icon_glyph_size_sm;
                    let spinner = |ui: &mut egui::Ui, rect: egui::Rect, _: egui::Color32| {
                        Spinner::new()
                            .size(size.value())
                            .color(theme.spinner_indicator().to_egui())
                            .paint_in(ui, theme, rect);
                    };
                    let mut apply = Button::new(if busy {
                        t("explorer.props.perm.applying")
                    } else {
                        t("explorer.props.perm.apply")
                    })
                    .variant(ButtonVariant::Primary)
                    .size(ControlSize::Sm)
                    .enabled(!busy && !failed);
                    if busy {
                        apply = apply.leading_icon(&spinner).leading_icon_size(size);
                    }
                    apply.show(ui, theme);
                    Button::new(t("button.cancel"))
                        .variant(ButtonVariant::Ghost)
                        .size(ControlSize::Sm)
                        .enabled(!busy)
                        .show(ui, theme);
                },
            );
        });
    ui.add_space(theme.spacing_xs.value());
}

fn grid(ui: &mut egui::Ui, theme: &Theme, bits: u32, busy: bool) {
    let cell = theme.explorer_props_perm_cell_width().value();
    let row_h = theme.explorer_props_row_min_height().value();
    let caption = theme.font_size_caption.value();
    // 본체 `perm_edit::centered` 와 같다. 머리 글자는 높이 0, 체크박스는 행 높이 가운데.
    let slot = |ui: &mut egui::Ui, h: f32, content_h: f32, add: &mut dyn FnMut(&mut egui::Ui)| {
        ui.allocate_ui_with_layout(
            egui::vec2(cell, h),
            egui::Layout::top_down(egui::Align::Center),
            |ui| {
                ui.set_min_size(egui::vec2(cell, h));
                ui.add_space(((h - content_h) / 2.0).max(0.0));
                add(ui);
            },
        );
    };
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(0.0, theme.spacing_xs.value());
        ui.horizontal_top(|ui| {
            ui.add_space(cell);
            for key in [
                "explorer.props.perm.read",
                "explorer.props.perm.write",
                "explorer.props.perm.execute",
            ] {
                slot(ui, 0.0, 0.0, &mut |ui| {
                    ui.label(
                        egui::RichText::new(t(key))
                            .size(caption)
                            .color(theme.text_muted().to_egui()),
                    );
                });
            }
        });
        for (role, shift) in [
            ("explorer.props.perm.owner", 6u32),
            ("explorer.props.perm.group", 3),
            ("explorer.props.perm.others", 0),
        ] {
            ui.horizontal(|ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(cell, row_h),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.set_min_size(egui::vec2(cell, row_h));
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(t(role))
                                    .size(caption)
                                    .color(theme.explorer_props_value_fg().to_egui()),
                            )
                            .truncate(),
                        );
                    },
                );
                for bit in [4u32, 2, 1] {
                    let mut on = bits & (bit << shift) != 0;
                    slot(ui, row_h, theme.checkbox_size().value(), &mut |ui| {
                        tasty_ui_widgets::checkbox(ui, theme, &mut on, "", !busy);
                    });
                }
            });
        }
    });
}

fn muted(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.add(
        egui::Label::new(
            egui::RichText::new(text)
                .size(theme.font_size_caption.value())
                .color(theme.text_muted().to_egui()),
        )
        .wrap(),
    );
}
