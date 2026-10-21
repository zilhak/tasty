//! 탐색기 Properties 의 동작 예제: 시각 형식별 시각 필드 · Open footer · 권한 편집 상자.
//! 시안 `gallery/explorer-ops-props-actions.jsx` 의 "Properties — time format · Open · permissions" 구역을 옮긴다.
//! 본체는 `explorer_properties` popup 과 그 `perm_edit` 하위 모듈이 같은 배치를 그린다.

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{PermBoxProps, PermBoxText, PermBoxValue};

use super::explorer_file_info::{Extras, field, mono, props_card_full, time, tr, warn};
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
                    lang: Option<&'static str>,
                    glyph: (MockGlyph, &str),
                    bits: u32,
                    folder: bool,
                    special: bool,
                    state: BoxState| {
            let edit = move |ui: &mut egui::Ui| {
                perm_box(ui, theme, lang, Some(bits), folder, special, state)
            };
            cluster(ui, theme, label, |ui| {
                card(
                    ui,
                    theme,
                    glyph,
                    &[
                        field(
                            "explorer.properties.kind",
                            tr(
                                lang,
                                if folder {
                                    "explorer.type.folder"
                                } else {
                                    "explorer.type.file"
                                },
                            ),
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
                        lang,
                    },
                );
            });
        };
        unix(
            ui,
            "editing",
            None,
            (icons::FILE, "deploy.env"),
            0o644,
            false,
            false,
            BoxState::Editing,
        );
        unix(
            ui,
            "folder — applying",
            None,
            (icons::FOLDER, "shared"),
            0o775,
            true,
            true,
            BoxState::Applying,
        );
        unix(
            ui,
            "failed — real mode re-read",
            None,
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
        for (label, value, state) in [
            (
                "Windows — read-only attribute",
                "read-only: yes",
                BoxState::Editing,
            ),
            (
                "Windows — failed",
                "read-only: no",
                BoxState::Failed("Access is denied. (os error 5)"),
            ),
        ] {
            let edit =
                move |ui: &mut egui::Ui| perm_box(ui, theme, None, None, false, false, state);
            cluster(ui, theme, label, |ui| {
                card(
                    ui,
                    theme,
                    (icons::FILE, "report.docx"),
                    &[mono("explorer.properties.permissions", value, false)],
                    None,
                    Extras {
                        under: Some(&edit),
                        footer: Some(true),
                        lang: None,
                    },
                );
            });
        }
        // 긴 열 머리·역할명은 칸 안에서 줄바꿈한다(시안 "ko — editor" · "ja — editor").
        unix(
            ui,
            "ko — editor",
            Some("ko"),
            (icons::FILE, "배포.env"),
            0o644,
            false,
            false,
            BoxState::Editing,
        );
        unix(
            ui,
            "ja — editor",
            Some("ja"),
            (icons::FOLDER, "共有"),
            0o755,
            true,
            false,
            BoxState::Editing,
        );
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
                "under the row, spans the content width with the label column (330 at the 360 popup) · Unix and Windows alike · 1px explorer-props-perm-border · radius · padding space-sm · gap space-sm",
            ),
            (
                "grid",
                "role column + Read · Write · Execute · cell explorer-props-perm-cell-width 56 (4 × 56 + padding + border = 242 ≤ 330) · rows row-min-height · grid left-aligned",
            ),
            (
                "long words",
                "heads centred and bottom-aligned, role names left-aligned · both wrap inside the 56 cell, never ellipsised",
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

/// 본체 `perm_edit::draw` 와 같은 공용 `perm_box`. `bits` 가 None 이면 Windows 읽기 전용 상자다.
/// 편집 예제는 기타의 비트를 끈 상태(바뀜)로, 실패 예제는 다시 읽은 원래 값으로 보인다.
fn perm_box(
    ui: &mut egui::Ui,
    theme: &Theme,
    lang: Option<&'static str>,
    bits: Option<u32>,
    folder: bool,
    special: bool,
    state: BoxState,
) {
    let tl = |key| tr(lang, key);
    let failed = matches!(state, BoxState::Failed(_));
    let mut notes = Vec::new();
    if folder {
        notes.push(tl("explorer.props.perm.folder_only"));
    }
    if special {
        notes.push(tl("explorer.props.perm.special_kept"));
    }
    let note = notes.join(" ");
    let mut shown = bits.map_or(0, |from| if failed { from } else { from & !0o7 });
    let mut read_only = !failed;
    let value = if bits.is_some() {
        PermBoxValue::Mode(&mut shown)
    } else {
        PermBoxValue::ReadOnly(&mut read_only)
    };
    let text = PermBoxText {
        heads: [
            tl("explorer.props.perm.read"),
            tl("explorer.props.perm.write"),
            tl("explorer.props.perm.execute"),
        ],
        roles: [
            tl("explorer.props.perm.owner"),
            tl("explorer.props.perm.group"),
            tl("explorer.props.perm.others"),
        ],
        read_only: tl("explorer.props.perm.read_only"),
        windows_hint: tl("explorer.props.perm.windows_hint"),
        failed: tl("explorer.props.perm.failed"),
        apply: tl("explorer.props.perm.apply"),
        applying: tl("explorer.props.perm.applying"),
        cancel: tl("button.cancel"),
    };
    // 예제는 상태를 고정해 그리므로 누른 버튼은 쓰지 않는다.
    tasty_ui_widgets::perm_box(
        ui,
        theme,
        PermBoxProps {
            text,
            value,
            note: (!note.is_empty()).then_some(note.as_str()),
            failure: match state {
                BoxState::Failed(reason) => Some(reason),
                _ => None,
            },
            busy: state == BoxState::Applying,
            changed: !failed,
        },
    );
}
