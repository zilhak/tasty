//! 접어도 경로가 넘칠 때 무엇이 먼저 줄어드는지 — 디자인 Spec "Path bar — what gives way".

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;

use super::path_bar::{PathKind, path_step};
use super::{FRAME_W, FpState, Variant, card};
use crate::catalog::spec::{self, StageVariant, TokenChip};

/// 폭 사다리 카드의 높이 — 디자인 Spec 의 측정 카드(`FilePickerFrame pathKind="longroot" w={440} h={300}`).
/// 높이는 배분에 영향이 없다.
const LADDER_H: LogicalPx = LogicalPx(300.0);
/// 디자인 폭 사다리(640 · 520 · 440 · 360 · 320)의 가운데 칸. 양 끝은 기본 카드 폭과 피커 바닥 폭이다.
const LADDER_520: LogicalPx = LogicalPx(520.0);
/// 디자인 폭 사다리의 440 칸.
const LADDER_440: LogicalPx = LogicalPx(440.0);
/// 디자인 폭 사다리의 360 칸.
const LADDER_360: LogicalPx = LogicalPx(360.0);

/// `crumb_alloc::plan` 이 고른 단계를 디자인 Spec 의 단계 이름으로 적는다. 단계 번호는 덜 접힌
/// 구성부터 센 순번이라, 조상을 하나씩 접는 1단계가 성분 수만큼 늘어난다.
fn step_label(n: usize, step: usize) -> String {
    let foldable = n.saturating_sub(3);
    if step == 0 {
        return "fits — nothing folds".to_owned();
    }
    if n >= 3 && step <= foldable {
        return if step == 1 {
            "step 1 · 1 ancestor folded".to_owned()
        } else {
            format!("step 1 · {step} ancestors folded")
        };
    }
    let rest = if n >= 3 { step - foldable } else { step + 2 };
    match rest {
        1 => "step 2 · parent shrinks to its floor",
        2 => "step 3 · current shrinks to its floor",
        3 => "step 4 · root › … › current",
        _ => "step 5 · … › current",
    }
    .to_owned()
}

/// 측정 카드 한 장 — 라벨은 그 카드 폭에서 `plan` 이 돌려준 단계다.
fn ladder_card(ui: &mut egui::Ui, theme: &Theme, kind: &str, v: Variant) {
    let (n, step) = path_step(ui, theme, v);
    let label = format!("{} · {kind} — {}", v.w.value(), step_label(n, step));
    spec::cluster(ui, theme, &label, |ui| card(ui, theme, v));
}

pub fn draw_path_fit(ui: &mut egui::Ui, theme: &Theme) {
    let open = Variant::open(FpState::Loaded, false, false);
    let floor = theme.fp_popup_min_width();
    let widths = [FRAME_W, LADDER_520, LADDER_440, LADDER_360, floor];
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        for w in widths {
            ladder_card(
                ui,
                theme,
                "longtwo",
                open.path(PathKind::LongTwo).sized(w, LADDER_H),
            );
        }
        ladder_card(
            ui,
            theme,
            "longroot",
            open.path(PathKind::LongRoot).sized(floor, LADDER_H),
        );
    });

    spec::meta(
        ui,
        theme,
        &[
            (
                "measure",
                "path-bar width − Up − Refresh − gaps (not the popup width)",
            ),
            (
                "specimens",
                "measured only — a width ladder: longtwo at 640 · 520 · 440 · 360 · 320, longroot at 320, each labelled with the step crumb_alloc::plan returns",
            ),
            (
                "crumb type",
                "11 · font-size-caption · current = text-primary, no extra weight",
            ),
            ("1 fold", "ancestors → … menu, one per step"),
            ("2 parent", "180 → 64 (fp-crumb-min-width)"),
            ("3 current", "180 → 96 (fp-crumb-current-min-width)"),
            ("4 parent folds", "root › … › current"),
            ("5 root folds", "… › current — the design floor"),
            ("grow back", "floor + 8 (fp-bar-hysteresis)"),
            (
                "current ellipsis",
                "at the FRONT (…-bbbb) — the tail names the folder",
            ),
            ("ancestor ellipsis", "at the tail"),
            (
                "picker floor",
                "fp-popup-min-width 320 — the owning surface always wins",
            ),
        ],
        &[
            TokenChip::without_color("fp-crumb-max-width", "cap (approved, unchanged)"),
            TokenChip::without_color("fp-crumb-min-width", "ancestor floor"),
            TokenChip::without_color("fp-crumb-current-min-width", "current-folder floor"),
            TokenChip::without_color("fp-bar-hysteresis", "grow-back margin"),
            TokenChip::without_color("fp-popup-min-width", "popup floor"),
        ],
    );

    spec::do_(
        ui,
        theme,
        "Do keep the … menu in place through every step: hidden ancestors enter it in path \
         order, so steps 4 and 5 just prepend the parent and the root. Its hit area never \
         exceeds its painted box.",
    );
    spec::dont(
        ui,
        theme,
        "Don't buy width from the footer, the file-name input, the Refresh button, or the font \
         size. The path bar takes what is left over after those, and folds.",
    );
    spec::note(
        ui,
        theme,
        "These cards run the same allocation as the app (crumb_alloc::plan) on the width each \
         card actually has. Floors and caps are width tokens, so the order is the same at \
         ui_scale 0.85 / 1 / 1.2. Windows drives, UNC roots and remote user@host roots are \
         ordinary root crumbs — long ones fold at step 5.",
    );
}
