//! Handler › File Detectors · File Handlers 예제. 시안 `file_handlers.jsx`의 Pane을 시드 상태로 고정해
//! 그린다. 행·카드·머리는 본체와 같은 `tasty_ui_widgets::fh_*`를 쓰고, 상태 전이는 시안 kit을 옮긴
//! 갤러리 전용 모델이다(실제 레지스트리를 읽지 않는다).

use std::cell::RefCell;
use std::collections::HashMap;

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{
    FhCardLabels, FhLine, FhOrigin, FhRowView, FhSummarySeg, FhTrailing, Input, fh_card, fh_empty,
    fh_field, fh_intro, fh_list_head, fh_row,
};

use crate::catalog::spec::{self, StageVariant, TokenChip};
use crate::i18n::t;

/// 시안 `FhPair`의 한 칸 폭 `--tasty-size-560`. 설정 콘텐츠 컬럼 근사이며 공개 역할 토큰이 없다.
const PAIR_WIDTH: LogicalPx = LogicalPx(560.0);
/// 시안 좁은 패널 `--tasty-size-360`.
const NARROW_WIDTH: LogicalPx = LogicalPx(360.0);

mod copy;
mod model;
mod panes;

use copy::*;
use model::*;
use panes::*;

// ── Stage ───────────────────────────────────────────────────────────

enum PaneState {
    Det(DetState),
    Hdl(HdlState),
}

thread_local! {
    static PANES: RefCell<HashMap<(String, &'static str), PaneState>> = RefCell::new(HashMap::new());
}

/// 시안 `FhPair` + `FhPanel`: Mocha·Latte 두 칸에 같은 시드의 Pane을 각각 그린다.
fn pane(ui: &mut egui::Ui, theme: &Theme, key: &str, det: bool, seed: Seed) {
    let with_zoom =
        |base: Theme| Theme::with_colors_and_zoom(base.to_colors(), base.is_light, theme.ui_zoom);
    let themes = [
        ("Mocha", with_zoom(tasty_themes::mocha_fallback())),
        ("Latte", with_zoom(crate::host_shell::latte_theme())),
    ];
    let copy = if seed.de { copy_de() } else { copy_en() };
    spec::stage(ui, theme, StageVariant::Solo, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing =
                egui::vec2(theme.spacing_lg.value(), theme.spacing_lg.value());
            for (label, th) in &themes {
                let cell = PAIR_WIDTH.value() * theme.ui_zoom;
                // 시안 `flex: 1 1 560` + wrap: 두 칸이 문서 칸을 넘으면 Latte 칸이 아래 줄로 간다.
                spec::wrap_item(ui, |ui| {
                    egui::Frame::new()
                        .fill(th.bg_app().to_egui())
                        .corner_radius(th.corner_radius.value())
                        .inner_margin(egui::Margin::same(th.spacing_md.value() as i8))
                        .show(ui, |ui| {
                            ui.set_width(cell);
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = th.spacing_sm.value();
                                ui.label(
                                    egui::RichText::new(*label)
                                        .size(th.font_size_caption.value())
                                        .color(th.text_muted().to_egui()),
                                );
                                let panel_w = if seed.narrow {
                                    NARROW_WIDTH.value() * theme.ui_zoom
                                } else {
                                    cell
                                };
                                panel(ui, th, panel_w, |ui| {
                                    PANES.with(|p| {
                                        let mut p = p.borrow_mut();
                                        let st = p.entry((key.to_string(), *label)).or_insert_with(
                                            || {
                                                if det {
                                                    PaneState::Det(det_state(&seed))
                                                } else {
                                                    PaneState::Hdl(hdl_state(&seed))
                                                }
                                            },
                                        );
                                        ui.push_id((key, *label), |ui| match st {
                                            PaneState::Det(s) => detectors_pane(ui, th, &copy, s),
                                            PaneState::Hdl(s) => handlers_pane(ui, th, &copy, s),
                                        });
                                    });
                                });
                            });
                        });
                });
            }
        });
    });
}

/// 설정 콘텐츠 컬럼 — bg-panel · 1px border-frame · radius · padding space-lg.
fn panel(ui: &mut egui::Ui, th: &Theme, width: f32, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(th.bg_panel().to_egui())
        .stroke(egui::Stroke::new(
            th.border_width.value(),
            th.border_frame().to_egui(),
        ))
        .corner_radius(th.corner_radius.value())
        .inner_margin(egui::Margin::same(th.spacing_lg.value() as i8))
        .show(ui, |ui| {
            let inner = (width - 2.0 * (th.spacing_lg.value() + th.border_width.value())).max(0.0);
            ui.set_width(inner);
            ui.vertical(add);
        });
}

const ROW_SPECS: &[(&str, &str)] = &[
    (
        "row",
        "2 lines · padding space-sm space-xs · gap space-xs · bottom 1px separator",
    ),
    (
        "line 1",
        "min-height button-height-sm · gap space-sm · id mono body 13 semibold (ellipsis, full id in tooltip) · origin Tag · [prio N micro] · [pending Tag] · spacer · Switch · trailing slot",
    ),
    (
        "origin Tag(s)",
        "host / user neutral · plugin:<id> agent variant · detectors: one Tag per source, order host → plugin → user, gap space-sm; a rule-less user patch shows “user override” · collapse → one neutral “N sources” Tag (tooltip = full list) once the Tags would ellipsize the id",
    ),
    (
        "trailing slot",
        "any user contribution → Remove (ghost sm) · pending → Undo (ghost sm), same slot · no user contribution → lock glyph icon-size-sm, glyph-dim, tooltip “Provided by host, plugin:logkit”",
    ),
    (
        "line 2",
        "summary mono term-sm, one line, ellipsis, full value in tooltip · detectors: every source's rules in merge order “ext md, markdown · ext mdx” · handlers “<detector> → <action>”",
    ),
    (
        "switched off",
        "id text-secondary · summary text-muted (still editable)",
    ),
    (
        "sort",
        "detectors: registry order · handlers: priority ↑ then id",
    ),
];

const CARD_SPECS: &[(&str, &str)] = &[
    (
        "card",
        "file-handler-form-bg · 1px file-handler-form-border · radius · padding file-handler-form-padding 12 · row gap file-handler-form-row-gap 8",
    ),
    ("title", "mono micro caps, file-handler-form-label-fg"),
    (
        "field row",
        "label column settings-label-width 150 (wraps, body 13) · gap settings-label-gap 16 · control column flex ≥ field-width-lg; wraps under the label when the card is narrower",
    ),
    (
        "control column",
        "control (control-height 28) + one line under it, gap space-xs — priority error > info > caption",
    ),
    (
        "info line",
        "info glyph icon-size-sm + caption 12, file-handler-form-info-fg, role status; same box as the error line; live while typing (not only after Add)",
    ),
    (
        "widths",
        "text Inputs block (fill) · Detector Select file-handler-select-width 200 · Action Select file-handler-kind-width 160 · Priority Input file-handler-priority-width 90",
    ),
    (
        "buttons",
        "right-aligned, gap space-sm: Cancel ghost sm · Add primary sm",
    ),
    (
        "error",
        "alertCircle icon-size-sm + caption 12, file-handler-form-error-fg; replaces that field's caption; Input invalid. Cross-field error (no rule) sits above the button row",
    ),
];

fn row_tokens(th: &Theme) -> Vec<TokenChip> {
    vec![
        TokenChip::new(
            "file-handler-prio-fg",
            "→ text-muted",
            th.file_handler_prio_fg().to_egui(),
        ),
        TokenChip::new(
            "file-handler-summary-fg",
            "→ text-secondary",
            th.file_handler_summary_fg().to_egui(),
        ),
        TokenChip::new(
            "file-handler-summary-removed-fg",
            "→ text-muted",
            th.file_handler_summary_removed_fg().to_egui(),
        ),
        TokenChip::new(
            "file-handler-empty-fg",
            "→ text-muted",
            th.file_handler_empty_fg().to_egui(),
        ),
        TokenChip::new(
            "tag-disabled-bg",
            "added / removed on save Tag",
            th.tag_disabled_bg().to_egui(),
        ),
    ]
}

fn card_tokens(th: &Theme) -> Vec<TokenChip> {
    vec![
        TokenChip::new(
            "file-handler-form-bg",
            "→ surface-raised",
            th.file_handler_form_bg().to_egui(),
        ),
        TokenChip::new(
            "file-handler-form-border",
            "→ border-default",
            th.file_handler_form_border().to_egui(),
        ),
        TokenChip::new(
            "file-handler-form-label-fg",
            "→ text-secondary-raised",
            th.file_handler_form_label_fg().to_egui(),
        ),
        TokenChip::new(
            "file-handler-form-caption-fg",
            "→ text-secondary-raised",
            th.file_handler_form_caption_fg().to_egui(),
        ),
        TokenChip::new(
            "file-handler-form-info-fg",
            "→ text-secondary-raised",
            th.file_handler_form_info_fg().to_egui(),
        ),
        TokenChip::new(
            "file-handler-form-error-fg",
            "→ accent-danger",
            th.file_handler_form_error_fg().to_egui(),
        ),
        TokenChip::without_color("file-handler-priority-width", "→ field-width-xs 90"),
        TokenChip::without_color("file-handler-select-width", "→ field-width-lg 200"),
        TokenChip::without_color("file-handler-kind-width", "→ field-width-md 160"),
    ]
}

/// File Detectors 쪽 Spec — 목록, 카드, 검증, 여러 출처, 사용자 기여 삭제, patch, 대기, 빈 목록, 긴 문구.
pub fn draw_detectors(ui: &mut egui::Ui, theme: &Theme) {
    spec::spec(
        ui,
        theme,
        "Registered rows — two lines, no table",
        Some(
            "Line 1: id · one origin Tag per source · Switch · trailing slot. Line 2: every source's rules in merge order. Only rows with a user contribution have Remove; the rest show the padlock.",
        ),
    );
    pane(ui, theme, "det-list", true, Seed::default());
    spec::meta(ui, theme, ROW_SPECS, &row_tokens(theme));

    spec::spec(
        ui,
        theme,
        "Add card — open, empty",
        Some(
            "“Add detector” opens the card between the intro and the list and is disabled while it is open. Add stays disabled until Id has a value.",
        ),
    );
    pane(
        ui,
        theme,
        "det-adding",
        true,
        Seed {
            adding: true,
            ..Seed::default()
        },
    );
    spec::meta(ui, theme, CARD_SPECS, &card_tokens(theme));

    spec::spec(
        ui,
        theme,
        "Add card — validation errors",
        Some(
            "Format errors show after Add is pressed: id characters, and the cross-field “no extension and no glob” line above the buttons.",
        ),
    );
    pane(
        ui,
        theme,
        "det-attempted",
        true,
        Seed {
            adding: true,
            attempted: true,
            draft: Draft {
                id: "Docker_File",
                ..Draft::default()
            },
            ..Seed::default()
        },
    );

    spec::spec(
        ui,
        theme,
        "Several sources on one detector",
        Some(
            "markdown host + user rule · log host + plugin:logkit + user rule · notebook plugin:md-plus + user override (switched off, no user rule). In the narrow column the Tags collapse into one “N sources” Tag.",
        ),
    );
    pane(ui, theme, "det-sources", true, Seed::default());
    pane(
        ui,
        theme,
        "det-sources-narrow",
        true,
        Seed {
            narrow: true,
            ..Seed::default()
        },
    );

    spec::spec(
        ui,
        theme,
        "Remove a user contribution — the row stays",
        Some(
            "Only the user Tag and the user rule pieces are struck; the Switch is disabled and shows the state after Save. A user-only row strikes the whole id.",
        ),
    );
    pane(
        ui,
        theme,
        "det-unpatch",
        true,
        Seed {
            unpatch: true,
            ..Seed::default()
        },
    );
    pane(
        ui,
        theme,
        "det-unpatch-pending",
        true,
        Seed {
            unpatch: true,
            pending: true,
            ..Seed::default()
        },
    );

    spec::spec(
        ui,
        theme,
        "Add with an existing id — patch, not an error",
        Some(
            "An existing id shows the info line at once and the button reads Update detector. A registered $ id is patched the same way; a new $ id fails with the reserved error on Add.",
        ),
    );
    for (key, draft, attempted) in [
        (
            "det-patch-image",
            Draft {
                id: "image",
                ext: "heic",
                ..Draft::default()
            },
            false,
        ),
        (
            "det-patch-markdown",
            Draft {
                id: "markdown",
                ext: "mdx, mdown",
                ..Draft::default()
            },
            false,
        ),
        (
            "det-patch-directory",
            Draft {
                id: "$directory",
                glob: "**/.git",
                ..Draft::default()
            },
            false,
        ),
        (
            "det-reserved",
            Draft {
                id: "$foo",
                ext: "foo",
                ..Draft::default()
            },
            true,
        ),
    ] {
        pane(
            ui,
            theme,
            key,
            true,
            Seed {
                adding: true,
                attempted,
                draft,
                ..Seed::default()
            },
        );
    }
    pane(
        ui,
        theme,
        "det-patched",
        true,
        Seed {
            patched: true,
            ..Seed::default()
        },
    );

    spec::spec(
        ui,
        theme,
        "Pending — added on save · removed on save",
        Some(
            "Add closes the card and lists the entry with “added on save”; Remove strikes the id. Undo takes either back.",
        ),
    );
    pane(
        ui,
        theme,
        "det-pending",
        true,
        Seed {
            pending: true,
            ..Seed::default()
        },
    );

    spec::spec(ui, theme, "Empty list", None);
    pane(
        ui,
        theme,
        "det-empty",
        true,
        Seed {
            empty: true,
            ..Seed::default()
        },
    );

    spec::spec(ui, theme, "Long labels · narrow column", None);
    pane(
        ui,
        theme,
        "det-de-narrow",
        true,
        Seed {
            de: true,
            narrow: true,
            unpatch: true,
            ..Seed::default()
        },
    );
    spec::do_(
        ui,
        theme,
        "Keep the summary on one line and put the full value in the tooltip, so every row has the same height.",
    );
}

/// File Handlers 쪽 Spec — 목록, 카드, 검증, 동작 종류, 같은 이름 교체, 대기, 빈 목록, 긴 문구.
pub fn draw_file_handlers(ui: &mut egui::Ui, theme: &Theme) {
    spec::spec(
        ui,
        theme,
        "Registered rows — two lines, no table",
        Some(
            "Line 1: id · origin Tag · prio N · Switch · trailing slot. Line 2: <detector> → <action>. Rows sort by priority, then id.",
        ),
    );
    pane(ui, theme, "hdl-list", false, Seed::default());
    spec::meta(ui, theme, ROW_SPECS, &row_tokens(theme));

    spec::spec(
        ui,
        theme,
        "Add card — open, empty",
        Some(
            "Add stays disabled until Short name, Detector and the action's required field have a value. The Saved as caption shows the stored id.",
        ),
    );
    pane(
        ui,
        theme,
        "hdl-adding",
        false,
        Seed {
            adding: true,
            ..Seed::default()
        },
    );
    spec::meta(ui, theme, CARD_SPECS, &card_tokens(theme));

    spec::spec(
        ui,
        theme,
        "Add card — validation errors",
        Some(
            "Short name characters (≤ 32), priority not a whole number, IPC method not dotted — shown after Add.",
        ),
    );
    pane(
        ui,
        theme,
        "hdl-attempted",
        false,
        Seed {
            adding: true,
            attempted: true,
            draft: Draft {
                short: "docker.system",
                det: "dockerfile",
                prio: "1.5",
                surface: "text_view",
                ..Draft::default()
            },
            ..Seed::default()
        },
    );

    spec::spec(
        ui,
        theme,
        "Handler add card — action kinds",
        Some(
            "Open surface: Surface kind + Param key. IPC: Method. System: no rows; the Action caption says what it does.",
        ),
    );
    for (key, draft) in [
        (
            "hdl-kind-surface",
            Draft {
                short: "notes-view",
                det: "markdown",
                kind: 0,
                surface: "markdown_view",
                ..Draft::default()
            },
        ),
        (
            "hdl-kind-ipc",
            Draft {
                short: "notes-ipc",
                det: "markdown",
                prio: "40",
                kind: 1,
                method: "com.example.notes.open",
                ..Draft::default()
            },
        ),
        (
            "hdl-kind-system",
            Draft {
                short: "notes-os",
                det: "markdown",
                kind: 2,
                ..Draft::default()
            },
        ),
    ] {
        pane(
            ui,
            theme,
            key,
            false,
            Seed {
                adding: true,
                draft,
                ..Seed::default()
            },
        );
    }

    spec::spec(
        ui,
        theme,
        "Handler — same short name replaces",
        Some(
            "An existing user/<short> shows the info line in place of Saved as, and the button reads Update handler.",
        ),
    );
    pane(
        ui,
        theme,
        "hdl-replace",
        false,
        Seed {
            adding: true,
            draft: Draft {
                short: "dockerfile-system",
                det: "dockerfile",
                kind: 2,
                ..Draft::default()
            },
            ..Seed::default()
        },
    );

    spec::spec(ui, theme, "Pending — added on save · removed on save", None);
    pane(
        ui,
        theme,
        "hdl-pending",
        false,
        Seed {
            pending: true,
            ..Seed::default()
        },
    );

    spec::spec(ui, theme, "Empty list", None);
    pane(
        ui,
        theme,
        "hdl-empty",
        false,
        Seed {
            empty: true,
            ..Seed::default()
        },
    );

    spec::spec(
        ui,
        theme,
        "Long labels · narrow column",
        Some(
            "Labels wrap inside the 150 column. When the card is narrower than label + gap + field-width-lg the control drops under its label.",
        ),
    );
    pane(
        ui,
        theme,
        "hdl-de",
        false,
        Seed {
            de: true,
            adding: true,
            draft: Draft {
                short: "notizen",
                det: "markdown",
                kind: 0,
                ..Draft::default()
            },
            ..Seed::default()
        },
    );
    pane(
        ui,
        theme,
        "hdl-narrow",
        false,
        Seed {
            narrow: true,
            adding: true,
            draft: Draft {
                short: "notes-view",
                det: "markdown",
                kind: 1,
                method: "com.example.notes.open",
                ..Draft::default()
            },
            ..Seed::default()
        },
    );
    spec::dont(
        ui,
        theme,
        "Shrink the label column below settings-label-width to fit long labels; wrap them instead.",
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeds_follow_the_kit_states() {
        let un = det_state(&Seed {
            unpatch: true,
            ..Seed::default()
        });
        let ids: Vec<&str> = un
            .xs
            .iter()
            .filter(|x| x.pending == Some(DetPending::Unpatch))
            .map(|x| x.id.as_str())
            .collect();
        assert_eq!(ids, ["markdown", "log", "notebook"]);
        let pend = det_state(&Seed {
            pending: true,
            ..Seed::default()
        });
        assert_eq!(
            pend.xs
                .iter()
                .filter(|x| x.pending == Some(DetPending::Remove))
                .map(|x| x.id.as_str())
                .collect::<Vec<_>>(),
            ["dockerfile"]
        );
        let h = hdl_state(&Seed {
            draft: Draft {
                det: "dockerfile",
                ..Draft::default()
            },
            ..Seed::default()
        });
        assert_eq!(h.det, Some(5));
    }
}
