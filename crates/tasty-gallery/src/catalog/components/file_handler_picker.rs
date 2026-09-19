//! File handler picker — 디자인 `gallery/overlays-dialogs.jsx` §filehandler 의 10 Spec 미러.
//!
//! canonical 프레임은 `gallery/overlays-shared.jsx` 의 `FileHandlerFrame` 이다: 420px,
//! **headless**(프레임이 자기 헤더를 그린다), 헤더에 제목 + 형식 Tag + mono 경로,
//! 본문은 `Suggested` → `Recent` 두 그룹이 **한 목록** 안에 있고, 선택 행은
//! surface-active + 2px accent 좌측 인셋 바, plugin 은 출처 낱말과 글리프만 mauve,
//! footer 는 Cancel / Open.
//!
//! 상태 다섯: `Default`(감지됨) · `Recent`(두 그룹) · `Fallback`(전체 핸들러 + 1회성 안내)
//! · `Empty`(등록된 핸들러 0) · `Long`(목록 높이 상한 + 하단 페이드). `Mixed` 는 F2/F3
//! 도출 규칙(선언된 이름·아이콘 / 아무것도 없는 것 / 긴 id)을 한 목록에 섞어 보이는 변형.
//!
//! **F1 은 제거로 확정됐다** — 디자인 §filehandler "Footer — settled" 가 (b) 를 골랐고,
//! picker 는 순수 dispatcher 다(1회 열기, 아무것도 저장 안 함). 디자인 Spec 은 기각된 두
//! 읽기(체크박스 / 비활성 체크박스)를 결정 표본으로 남겨 두지만, 이 specimen 은 확정된
//! footer 하나만 싣는다 — 기각된 읽기를 렌더하면 "Always open" 문자열이 레포에 남는다.
//!
//! 본체(`src/adapters/ui/popup/file_handler_picker.rs`)와 코드를 공유하지 않는다(갤러리
//! 표본은 정적 렌더). 공유하는 것은 `tasty_ui_widgets::tokens` 의 `FH_*` 치수뿐이다.

mod frame;

use tasty_type_appearance::theme::Theme;

use crate::catalog::spec::{self, StageVariant, TokenChip};
use crate::catalog::widgets::dialog as kit;
use frame::{FrameState, fh_footer, frame, header_card};
use tasty_ui_widgets::file_handler as fh_model;
use tasty_ui_widgets::tokens::FH_FRAME_WIDTH;

// ── Spec 본문 (catalog.rs 가 Spec 하나당 하나씩 부른다) ────────────────────────
/// Spec 1 — "Open with… — pick a handler".
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

/// Spec 2 — "Detected format — one Tag in the header".
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

/// Spec 3 — "Recent — a second group in the same list".
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

/// Spec 3b — "Relative time — six words and then a date".
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

/// Spec 3c — "Header path — cut whole segments, measured not counted".
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
        "Sample A is drawn cut. It is 70 characters and the budget is 65, so it overflows by 5 \
         characters — 30px at 6px a cell — and no longer shows the untouched case. The sample \
         string is a design value and is left as it stands; a replacement of 65 characters or \
         fewer is the design's to choose.",
    );
    spec::do_(
        ui,
        theme,
        "Do measure the line box against the font as it is laid out, not as the font file \
         declares it. D2Coding at 11px advances 5.5556px per glyph, but each advance is rounded \
         to a whole pixel when the line is built, so a character costs 6px. 390 ÷ 6 = 65 is the \
         derived cap a screen that cannot measure falls back to, so the two never disagree.",
    );
    spec::dont(
        ui,
        theme,
        "Don't cut inside a directory name to save two characters. A half-written segment reads \
         as a directory that does not exist, which is worse than one fewer level of context.",
    );
}

/// 경로 컷 Spec 의 세 표본 — **자르기 전** 값이다.
///
/// specimen 은 잘린 결과를 적어 두지 않고 본체와 **같은 함수**에 넣어 그 자리에서
/// 자른다. 결과를 적어 두면 규칙이 바뀌어도 그림은 안 바뀌어서, 이 Spec 이 규칙을
/// 보여주는 것이 아니라 규칙이 한때 그랬다는 기록이 된다.
const PATH_SAMPLES: [&str; 3] = [
    "work/tasty/crates/tasty-gallery/src/catalog/components/file_handler.rs",
    "/home/maya/src/tasty-main/crates/tasty-gallery/src/catalog/components/file_handler_picker.rs",
    "quarterly-revenue-reconciliation-draft-final-v3-reviewed-by-finance.xlsx",
];

/// specimen 의 예산 — 갤러리는 실제 헤더 폰트를 재지 않고 **파생 상한**을 쓴다.
///
/// Spec 이 보이려는 것은 "못 잴 때 어디로 떨어지는가" 를 포함한 규칙 전체이고, 그
/// 갈래의 값이 이것이다. 측정 갈래는 본체가 매 프레임 돈다.
const PATH_BUDGET: usize = tasty_ui_widgets::tokens::FH_TARGET_ELIDE_FALLBACK;

/// Spec 4 — "No suggestions — the whole catalog, one time only".
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

/// Spec 5 — "Empty — nothing to pick from".
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

/// Spec 6 — "Long list — cap the height, show the cut".
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

/// Spec 7 — "One header, not two".
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

/// Spec 8 — "Footer — settled: Cancel / Open, nothing else".
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

/// Spec 9 — "Rows — the icon and the name are derived, not stored".
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

/// Spec 10 — "Default Tag, and what the picker means now".
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

    /// 경로 컷 Spec 의 cluster 라벨이 **참인지** 본다.
    ///
    /// 라벨은 "sample A — 70 chars, 5 over the budget" · "92 → 61" ·
    /// "one 72-char segment → 65" 라고 말한다. 그림은 그 말과 별개로 그려지므로, 말이
    /// 낡아도 아무것도 안 빨개진다 — 여기서 표본의 길이와 함수의 출력 길이를 라벨과
    /// 맞물려 고정한다.
    #[test]
    fn the_path_cut_labels_describe_what_the_specimen_draws() {
        let [over, segment, one_piece] = PATH_SAMPLES;

        // 표본 A 는 예산을 넘는다 — 라벨이 말하는 5 자가 그 차다.
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
