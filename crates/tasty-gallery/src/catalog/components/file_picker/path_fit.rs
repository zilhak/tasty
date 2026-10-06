//! 접어도 경로가 넘칠 때 무엇이 먼저 줄어드는지 — 디자인 Spec "Path bar — what gives way".

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;

use super::path_bar::PathKind;
use super::{FpState, SaveState, Variant, card};
use crate::catalog::spec::{self, StageVariant, TokenChip};

/// 디자인 Spec 의 좁은 카드(`FilePickerFrame w={400} h={360}`). 대응 역할 토큰이 없는 무대 치수다.
const NARROW_W: LogicalPx = LogicalPx(400.0);
const NARROW_H: LogicalPx = LogicalPx(360.0);
/// 디자인 Spec 의 UNC root 카드(`FilePickerFrame w={440} h={300}`).
const ROOT_W: LogicalPx = LogicalPx(440.0);
const ROOT_H: LogicalPx = LogicalPx(300.0);

pub fn draw_path_fit(ui: &mut egui::Ui, theme: &Theme) {
    let open = Variant::open(FpState::Loaded, false, false);
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        spec::cluster(
            ui,
            theme,
            "640×480 · two 53-char segments — fits unfolded, each capped at 180",
            |ui| {
                card(ui, theme, open.path(PathKind::LongTwo));
            },
        );
        spec::cluster(
            ui,
            theme,
            "400×360 · two 53-char segments — ancestors fold, parent at its 64 floor",
            |ui| {
                card(
                    ui,
                    theme,
                    open.path(PathKind::LongTwo).sized(NARROW_W, NARROW_H),
                );
            },
        );
        spec::cluster(
            ui,
            theme,
            "440×300 · long UNC root — capped at 180, nothing folds",
            |ui| {
                card(
                    ui,
                    theme,
                    open.path(PathKind::LongRoot).sized(ROOT_W, ROOT_H),
                );
            },
        );
        spec::cluster(
            ui,
            theme,
            "400×360 · save / overwrite — same rule, footer untouched",
            |ui| {
                card(
                    ui,
                    theme,
                    Variant::save(SaveState::Picked, false)
                        .path(PathKind::LongTwo)
                        .sized(NARROW_W, NARROW_H),
                );
            },
        );
    });

    spec::meta(
        ui,
        theme,
        &[
            (
                "measure",
                "path-bar width − trailing buttons − gap (not the popup width)",
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
