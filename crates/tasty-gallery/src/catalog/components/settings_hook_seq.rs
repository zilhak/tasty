//! Settings › Handler › Hook Handlers — IpcSequence 문자열 편집기(시안 `HookSeqEditorG`).
//! 본체와 같은 `sequence_editor` 를 그린다. 갤러리는 해석기를 두지 않으므로 시안처럼 상태마다 고정
//! 결과(normal · error · empty)를 보인다.

use std::cell::RefCell;

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{SequenceEditorError, SequenceEditorView, TagVariant, sequence_editor, tag};

use crate::catalog::icons;
use crate::catalog::spec::{StageVariant, TokenChip, meta, stage};

/// 시안 편집기 카드 폭 `--tasty-size-460`. 공개 역할 토큰이 없어 갤러리 무대 치수로 둔다.
const CARD_WIDTH: LogicalPx = LogicalPx(460.0);

#[derive(Clone, Copy, PartialEq, Eq)]
enum SeqState {
    Normal,
    Error,
    Empty,
}

const STATES: [SeqState; 3] = [SeqState::Normal, SeqState::Error, SeqState::Empty];

fn seed(state: SeqState) -> String {
    match state {
        SeqState::Normal => "system.info\nnotification.send {\"body\":\"${body.branch}\",\"title\":\"Build ${body.status}\"}\nworkspace.create {\"name\":\"ci-${body.run}\"}".into(),
        SeqState::Error => "# notify\nsystem.info\nnotification.send {\"title\": \"Build\", body: 1}".into(),
        SeqState::Empty => String::new(),
    }
}

thread_local! {
    // 상태 셋 × 테마 둘의 편집 버퍼.
    static BUFS: RefCell<Option<Vec<String>>> = const { RefCell::new(None) };
}

fn editor_card(
    ui: &mut egui::Ui,
    th: &Theme,
    state: SeqState,
    salt: (usize, &str),
    buf: &mut String,
) {
    let alert = |ui: &mut egui::Ui, rect: egui::Rect, c: egui::Color32| {
        icons::ALERT_CIRCLE
            .image(rect.width(), c)
            .paint_at(ui, rect);
    };
    let check = |text: &str| -> Result<usize, SequenceEditorError> {
        match state {
            SeqState::Error => Err(SequenceEditorError {
                line: 3,
                sentence: "Line 3, column 38: invalid params JSON.".into(),
                reason: Some("key must be a string".into()),
            }),
            _ => Ok(text
                .lines()
                .filter(|l| !l.trim().is_empty() && !l.trim().starts_with('#'))
                .count()),
        }
    };
    let view = SequenceEditorView {
        placeholder: "system.info",
        help: "One call per line: method, then optional JSON params. Lines starting with # are skipped and are not kept.",
        empty_note: "No calls. The handler does nothing.",
        cancel: "Cancel",
        apply: "Apply",
        alert_icon: &alert,
        check: &check,
    };
    let sm = th.spacing_sm.value() as i8;
    let md = th.spacing_md.value() as i8;
    egui::Frame::new()
        .fill(th.bg_panel().to_egui())
        .stroke(egui::Stroke::new(
            th.border_width.value(),
            th.border_frame().to_egui(),
        ))
        .corner_radius(th.corner_radius.value())
        .show(ui, |ui| {
            ui.set_width(CARD_WIDTH.value() - th.border_width.value() * 2.0);
            ui.spacing_mut().item_spacing.y = 0.0;
            egui::Frame::new()
                .inner_margin(egui::Margin {
                    left: md,
                    right: md,
                    top: sm,
                    bottom: 0,
                })
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = th.spacing_sm.value();
                        let mono = |s: &str, c: egui::Color32| {
                            egui::RichText::new(s)
                                .monospace()
                                .size(th.font_size_term_sm.value())
                                .color(c)
                        };
                        ui.label(mono("on_webhook", th.text_secondary().to_egui()));
                        ui.label(mono("ci-notify", th.text_primary().to_egui()));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            tag(ui, th, "you", TagVariant::Default, false);
                        });
                    });
                });
            egui::Frame::new()
                .inner_margin(egui::Margin {
                    left: md,
                    right: md,
                    top: sm,
                    bottom: md,
                })
                .show(ui, |ui| {
                    sequence_editor(ui, th, ("gallery_hook_seq", salt.0, salt.1), &view, buf);
                });
        });
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    let with_zoom =
        |base: Theme| Theme::with_colors_and_zoom(base.to_colors(), base.is_light, theme.ui_zoom);
    let themes = [
        ("Mocha", with_zoom(tasty_themes::mocha_fallback())),
        ("Latte", with_zoom(crate::host_shell::latte_theme())),
    ];
    BUFS.with(|b| {
        let mut slot = b.borrow_mut();
        let bufs =
            slot.get_or_insert_with(|| STATES.iter().flat_map(|s| [seed(*s), seed(*s)]).collect());
        // 시안은 상태마다 Stage 하나에 Mocha·Latte 짝을 둔다.
        for (si, (state, pair)) in STATES.iter().zip(bufs.chunks_mut(2)).enumerate() {
            stage(ui, theme, StageVariant::Wrap, |ui| {
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = theme.spacing_lg.value();
                    for ((label, th), buf) in themes.iter().zip(pair.iter_mut()) {
                        egui::Frame::new()
                            .fill(th.bg_app().to_egui())
                            .corner_radius(th.corner_radius.value())
                            .inner_margin(egui::Margin::same(th.spacing_md.value() as i8))
                            .show(ui, |ui| {
                                ui.vertical(|ui| {
                                    ui.spacing_mut().item_spacing.y = th.spacing_sm.value();
                                    ui.label(
                                        egui::RichText::new(*label)
                                            .size(th.font_size_caption.value())
                                            .color(th.text_muted().to_egui()),
                                    );
                                    editor_card(ui, th, *state, (si, label), buf);
                                });
                            });
                    }
                });
            });
        }
    });
    meta(
        ui,
        theme,
        &[
            (
                "placement",
                "inline — the row's second line becomes the editor; one row open at a time",
            ),
            (
                "field",
                "CodeArea · minRows 4 · grows to codearea-max-height 200, then scrolls · no wrap",
            ),
            (
                "help",
                "text-muted caption: One call per line: method, then optional JSON params. Lines starting with # are skipped and are not kept.",
            ),
            ("parse", "every change · first error only"),
            (
                "error line",
                "alertCircle + sentence in accent-danger caption · parser reason after it, mono caption text-muted (untranslated) · gutter number danger + tinted band",
            ),
            (
                "error copy",
                "Line {line}: a method name is required before the params. · Line {line}: the method name contains a control character. · Line {line}, column {column}: invalid params JSON.",
            ),
            (
                "empty",
                "allowed · note: No calls. The handler does nothing.",
            ),
            (
                "buttons",
                "right-aligned · Cancel ghost sm · Apply secondary sm (disabled while an error stands)",
            ),
            ("save flow", "Apply → tab draft → Settings Save"),
            ("keys", "Mod+Enter Apply · Esc Cancel · Enter newline"),
            (
                "reopen",
                "comments / blank lines are gone, JSON compact with sorted keys — the help line says so; no extra notice",
            ),
        ],
        &[
            TokenChip::new(
                "codearea-gutter-bg",
                "→ bg-sidebar",
                theme.codearea_gutter_bg().to_egui(),
            ),
            TokenChip::new(
                "codearea-gutter-fg",
                "→ text-muted",
                theme.codearea_gutter_fg().to_egui(),
            ),
            TokenChip::new(
                "codearea-error-fg",
                "→ accent-danger",
                theme.codearea_error_fg().to_egui(),
            ),
            TokenChip::without_color("codearea-max-height", "→ size-200"),
        ],
    );
}
