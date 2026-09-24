//! 파일 핸들러 선택 화면의 정적 예제. 추천·최근 목록, 전체 목록, 빈 상태를 보여준다.
//! 본체 뷰와 코드를 공유하지 않으며 FH_* 치수와 경로 생략 함수는 공용 크레이트에서 쓴다.
//! 선택은 한 번 열기에만 적용하고 기본 연결을 저장하지 않는다.

mod frame;

use tasty_type_appearance::theme::Theme;

use crate::catalog::spec::{self, StageVariant, TokenChip};
use crate::catalog::widgets::dialog as kit;
use frame::{FrameState, fh_footer, frame, header_card};
use tasty_ui_widgets::file_handler as fh_model;
use tasty_ui_widgets::tokens::FH_FRAME_WIDTH;

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        frame(ui, theme, FrameState::Default, true);
    });
    spec::meta(
        ui,
        theme,
        &[
            ("width", "420px"),
            ("header", "title + format Tag · path (mono 11)"),
            ("rows", "icon · name · origin"),
            ("default", "Tag accent"),
            ("selected", "2px accent left bar"),
            ("footer", "Cancel (ghost) · Open (primary)"),
        ],
        &[
            TokenChip::new(
                "surface-active",
                "selected",
                theme.surface_active().to_egui(),
            ),
            TokenChip::new(
                "accent-primary",
                "select bar",
                theme.accent_primary().to_egui(),
            ),
            TokenChip::new(
                "accent-agent",
                "plugin handler",
                theme.accent_agent().to_egui(),
            ),
        ],
    );
    spec::note(
        ui,
        theme,
        "Plugin handlers are marked with the mauve agent accent — you always know a third \
         party is handling the open.",
    );
}

pub fn draw_format(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        spec::cluster(ui, theme, "detected", |ui| {
            frame(ui, theme, FrameState::Default, true);
        });
        spec::cluster(ui, theme, "not detected → fallback", |ui| {
            frame(ui, theme, FrameState::Fallback, true);
        });
    });
    spec::meta(
        ui,
        theme,
        &[
            ("place", "title row, right-aligned"),
            ("detected", "Tag accent"),
            ("unknown", "Tag default (outline)"),
            ("path", "mono 11 · text-muted"),
            (
                "long path",
                "elide at the FRONT in the model, then render LTR",
            ),
        ],
        &[
            TokenChip::new(
                "accent-primary",
                "format Tag",
                theme.accent_primary().to_egui(),
            ),
            TokenChip::new("text-muted", "path ink", theme.text_muted().to_egui()),
        ],
    );
    spec::do_(
        ui,
        theme,
        "Do cut long paths at the front — the filename is the tail and the tail identifies \
         the file. Elide in the model (drop leading segments, prefix …/) and render the \
         result left-to-right.",
    );
}

pub fn draw_recent(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        frame(ui, theme, FrameState::Recent, true);
    });
    spec::meta(
        ui,
        theme,
        &[
            ("groups", "Suggested → rule → Recent"),
            ("group label", "11px uppercase, text-secondary"),
            ("count", "mono, next to the label"),
            ("caption", "11px muted, one line"),
            ("recent meta", "origin · relative time"),
            ("selection", "one row across BOTH groups"),
        ],
        &[
            TokenChip::new("separator", "group rule", theme.separator.to_egui()),
            TokenChip::new(
                "text-secondary",
                "group label",
                theme.text_secondary().to_egui(),
            ),
            TokenChip::new("text-muted", "caption / time", theme.text_muted().to_egui()),
        ],
    );
    spec::dont(
        ui,
        theme,
        "Don't split the frame into two side-by-side lists. Two 200px columns lose the origin \
         line, and the eye has to compare across a gutter to pick one thing.",
    );
}

pub fn draw_when(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        frame(ui, theme, FrameState::Recent, true);
    });
    spec::meta(
        ui,
        theme,
        &[
            ("< 60 s", "just now"),
            ("< 60 min", "{n}m ago"),
            ("< 24 h", "{n}h ago"),
            ("24–48 h", "yesterday"),
            ("2–7 d", "{n}d ago — ceiling at 7"),
            ("≥ 7 d", "YYYY-MM-DD — no phrase key"),
            ("n", "integer floor, never rounded up"),
            ("column", "reserved at fh-when-width (56px)"),
            ("recompute", "every frame — midnight flips it live"),
        ],
        &[TokenChip::new(
            "text-muted",
            "the whole slot",
            theme.text_muted().to_egui(),
        )],
    );
    spec::note(
        ui,
        theme,
        "The date is deliberately not a translated phrase. Past a week the number stops helping \
         you choose a handler, and a bare YYYY-MM-DD is bounded at ten characters, reads the same \
         in every locale, and has no plural rule to get wrong. That bound is what the column \
         reserves — which is why the id beside it never reflows when a row crosses a boundary.",
    );
    spec::dont(
        ui,
        theme,
        "Don't add {n}w ago or {n}mo ago, and don't shrink or drop the time to make room. A \
         partially cut timestamp reads as a different timestamp; the id is the piece that gives \
         way, and it already elides from the front.",
    );
}

pub fn draw_path_cut(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        for (label, raw) in [
            "sample A — 70 chars, 5 over the budget",
            "92 → 61 — whole segments dropped",
            "one 72-char segment → 65",
        ]
        .into_iter()
        .zip(PATH_SAMPLES)
        {
            spec::cluster(ui, theme, label, |ui| {
                header_card(ui, theme, &fh_model::elide_target_front(raw, PATH_BUDGET));
            });
        }
    });
    spec::meta(
        ui,
        theme,
        &[
            ("line box", "420 − 1×2 border − 14×2 pad = 390px"),
            ("cell", "6px laid out — 5.5556px nominal, rounded up"),
            ("budget", "measured: 390px ÷ one laid-out cell"),
            ("fallback", "65 chars, only when it cannot measure"),
            ("first rule", "drop a whole leading segment, prefix …/"),
            (
                "second rule",
                "characters — only if one segment is too long",
            ),
            ("direction", "front, because the filename is the tail"),
        ],
        &[TokenChip::new(
            "text-muted",
            "mono path line",
            theme.text_muted().to_egui(),
        )],
    );
    spec::note(
        ui,
        theme,
        "Sample A has 70 characters, exceeding this example’s 65-character limit by five. It therefore shows a shortened path. The example data is kept unchanged.",
    );
    spec::do_(
        ui,
        theme,
        "The host measures the available width with the actual font layout. A 65-character fallback corresponds to a 390px line and 6px cells with D2Coding at 11px. Other font metrics can produce a different measured limit.",
    );
    spec::dont(
        ui,
        theme,
        "Don't cut inside a directory name to save two characters. A half-written segment reads \
         as a directory that does not exist, which is worse than one fewer level of context.",
    );
}

/// 경로를 미리 잘라 저장하지 않고 공용 생략 함수에 넣어 비교한다.
const PATH_SAMPLES: [&str; 3] = [
    "work/tasty/crates/tasty-gallery/src/catalog/components/file_handler.rs",
    "/home/maya/src/tasty-main/crates/tasty-gallery/src/catalog/components/file_handler_picker.rs",
    "quarterly-revenue-reconciliation-draft-final-v3-reviewed-by-finance.xlsx",
];

/// 이 예제는 글꼴을 실측하지 않고 측정할 수 없을 때의 문자 수 상한을 사용한다.
const PATH_BUDGET: usize = tasty_ui_widgets::tokens::FH_TARGET_ELIDE_FALLBACK;

pub fn draw_fallback(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        frame(ui, theme, FrameState::Fallback, true);
    });
    spec::meta(
        ui,
        theme,
        &[
            ("label", "All handlers · accent-attention"),
            ("strip", "8/14 on surface-raised"),
            ("strip icon", "alertTriangle, attention"),
            ("promise", "one-time dispatch, no registration"),
        ],
        &[
            TokenChip::new(
                "accent-attention",
                "fallback tone",
                theme.accent_attention().to_egui(),
            ),
            TokenChip::new(
                "surface-raised",
                "notice strip",
                theme.surface_raised().to_egui(),
            ),
        ],
    );
    spec::note(
        ui,
        theme,
        "Attention-peach, not warning-yellow: nothing is wrong, the system just has no opinion.",
    );
}

pub fn draw_empty(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        frame(ui, theme, FrameState::Empty, true);
    });
    spec::meta(
        ui,
        theme,
        &[
            ("block", "32/14 padding, centered, 8px gap"),
            ("glyph", "text-disabled"),
            ("action", "Button secondary sm → Settings › Handlers"),
            ("Open", "disabled"),
        ],
        &[
            TokenChip::new(
                "text-disabled",
                "empty glyph",
                theme.text_disabled().to_egui(),
            ),
            TokenChip::new("text-muted", "instruction", theme.text_muted().to_egui()),
        ],
    );
    spec::do_(
        ui,
        theme,
        "Do keep the frame the same width and the footer intact — the empty state is the same \
         dialog, not a different one.",
    );
}

pub fn draw_long(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        frame(ui, theme, FrameState::Long, true);
    });
    spec::meta(
        ui,
        theme,
        &[
            ("list max-height", "264px"),
            ("overflow", "auto, list area only"),
            ("fade", "20px → bg-panel"),
            ("count", "mono, in the group label"),
        ],
        &[
            TokenChip::new("bg-panel", "fade target", theme.bg_panel().to_egui()),
            TokenChip::new(
                "separator",
                "header / footer rules",
                theme.separator.to_egui(),
            ),
        ],
    );
    spec::note(
        ui,
        theme,
        "Header and footer never scroll — the path, the format, and the actions stay put.",
    );
}

pub fn draw_headless(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        spec::cluster(ui, theme, "headless — one header", |ui| {
            frame(ui, theme, FrameState::Default, true);
        });
        spec::cluster(ui, theme, "titlebar + header — path twice", |ui| {
            frame(ui, theme, FrameState::Long, false);
        });
    });
    spec::meta(
        ui,
        theme,
        &[
            ("mode", "headless: true"),
            ("header owner", "the frame"),
            ("dismiss", "Esc · Cancel · titlebar X — not scrim click"),
            ("precedent", "Transfer progress"),
        ],
        &[
            TokenChip::new(
                "bg-sidebar",
                "titlebar fill (the rejected option)",
                theme.bg_sidebar().to_egui(),
            ),
            TokenChip::new("separator", "header rule", theme.separator.to_egui()),
        ],
    );
    spec::dont(
        ui,
        theme,
        "Don't pair the common titlebar with an in-frame header. The path gets cut two \
         different ways in one dialog.",
    );
}

pub fn draw_footer(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        kit::frame_card(ui, theme, FH_FRAME_WIDTH, kit::panel_fill(theme), |ui| {
            fh_footer(ui, theme, true);
        });
    });
    spec::meta(
        ui,
        theme,
        &[
            ("footer", "Cancel (ghost) · Open (primary)"),
            ("persistence", "none — dispatch is one-time"),
            ("default binding", "lives in Settings › Handlers, set there"),
            ("Open", "disabled until a row is selected"),
        ],
        &[
            TokenChip::new("separator", "footer rule", theme.separator.to_egui()),
            TokenChip::new("accent-primary", "Open", theme.accent_primary().to_egui()),
        ],
    );
    spec::note(
        ui,
        theme,
        "The picker is a pure dispatcher: it opens one file, one time, and stores nothing. \
         If a persistent “always open with” ever ships, it belongs in Settings › Handlers as a \
         visible, removable row — not as a checkbox on a transient dialog.",
    );
}

pub fn draw_rows(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        frame(ui, theme, FrameState::Mixed, true);
    });
    spec::meta(
        ui,
        theme,
        &[
            ("icon", "from action surface kind → icons.json"),
            ("icon fallback", "file"),
            ("name", "id segment after the last “/”"),
            ("name (raw id)", "mono — signals “no declared name”"),
            ("id line", "origin · id (mono 11), front-elided at 34 chars"),
            ("origin words", "built-in · you · plugin"),
            ("plugin", "mauve accent-agent on the origin word + glyph"),
        ],
        &[
            TokenChip::new(
                "accent-agent",
                "plugin origin",
                theme.accent_agent().to_egui(),
            ),
            TokenChip::new("text-muted", "id line", theme.text_muted().to_egui()),
        ],
    );
    spec::note(
        ui,
        theme,
        "The mauve stays on the origin word and the glyph, not on the name — a plugin's \
         handler is still named after what it does.",
    );
}

pub fn draw_default_tag(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        spec::cluster(
            ui,
            theme,
            "ambiguous / explicit — default Tag shown",
            |ui| {
                frame(ui, theme, FrameState::Default, true);
            },
        );
        spec::cluster(ui, theme, "no match — no default", |ui| {
            frame(ui, theme, FrameState::Fallback, true);
        });
    });
    spec::meta(
        ui,
        theme,
        &[
            ("single click", "select, frame stays"),
            ("double click", "select + open"),
            ("no selection", "Open disabled"),
            ("selection scope", "one row across all groups"),
            ("dismiss", "Esc · Cancel · titlebar X"),
            ("scrim click", "does NOT close"),
        ],
        &[
            TokenChip::new(
                "accent-primary",
                "default Tag fill",
                theme.accent_primary().to_egui(),
            ),
            TokenChip::new(
                "text-on-accent",
                "default Tag ink",
                theme.text_on_accent().to_egui(),
            ),
        ],
    );
    spec::note(
        ui,
        theme,
        "Keep the Tag. It answers “what happens if I just press Enter” — the question the \
         picker exists to ask.",
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 예제 라벨에 적힌 원본·생략 결과의 문자 수를 실제 함수 결과와 비교한다.
    #[test]
    fn the_path_cut_labels_describe_what_the_specimen_draws() {
        let [over, segment, one_piece] = PATH_SAMPLES;

        assert_eq!(over.chars().count(), 70);
        assert_eq!(over.chars().count() - PATH_BUDGET, 5);
        assert_ne!(fh_model::elide_target_front(over, PATH_BUDGET), over);

        assert_eq!(segment.chars().count(), 92);
        let cut = fh_model::elide_target_front(segment, PATH_BUDGET);
        assert_eq!(cut.chars().count(), 61);
        assert!(cut.starts_with("…/tasty-gallery/"), "{cut}");

        assert_eq!(one_piece.chars().count(), 72);
        assert!(
            !one_piece.contains('/'),
            "한 조각이어야 문자 컷 갈래로 간다"
        );
        let cut = fh_model::elide_target_front(one_piece, PATH_BUDGET);
        assert_eq!(cut.chars().count(), 65);
        assert!(cut.starts_with('…') && !cut.starts_with("…/"), "{cut}");
    }
}
