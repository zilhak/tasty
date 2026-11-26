//! Spec `row-focus-and-list-keys`: 행 포커스 링과 목록 키가 남긴 상태. 디자인 `overlays-windows` 의 같은 Spec 을 옮긴다.
//! 카드는 `FilePickerFrame` 의 `pick` · `focusRow` · `hoverRow` 와 같은 [`Variant`] 로 그린다.
use super::{FILES, FpState, Mode, Row, RowState, SaveState, Variant, card, row};
use crate::catalog::spec::{self, StageVariant, TokenChip};
use tasty_type_appearance::theme::Theme;

/// 갈래 이름과 변형. 표본과 시험이 함께 읽는다.
pub(super) type Captioned = (&'static str, Variant);

fn open() -> Variant {
    Variant::open(FpState::Loaded, false, false)
}

fn save() -> Variant {
    Variant::save(SaveState::New, false)
}

/// 폴더 모드 — 디자인 `mode="folder"`. 고른 행은 `pick` 이 정한다.
fn folder() -> Variant {
    let mut v = open();
    v.mode = Mode::Folder {
        selected: false,
        lang: super::folder::Language::En,
        long: false,
    };
    v
}

/// 무대의 두 카드 — 디자인 Stage.
pub(super) fn stage_cards() -> [Captioned; 2] {
    [
        (
            "Open · target spec · ↓ focuses and selects package.json · ring on the selected row · File name reads package.json · Open enabled",
            open()
                .picked(Some("package.json"))
                .row_focus("package.json"),
        ),
        (
            "Save · target spec · ↓ focuses package.json · ring only, no selection · File name keeps keybindings-2026-09-14.toml · Save unchanged",
            save().row_focus("package.json"),
        ),
    ]
}

/// 카드로 그리는 View 들 — (View 이름, View 설명, 카드들). 디자인 Views 순서다.
pub(super) fn views() -> Vec<(&'static str, &'static str, Vec<Captioned>)> {
    vec![
        (
            "fp-row-focus-entry",
            "Open mode · after entering a folder, going up or Refresh · then the first ↓",
            vec![
                (
                    "After navigation · no selection, row focus or ring · File name empty · Open disabled",
                    open().picked(None),
                ),
                (
                    "First ↓ · configs selected with the ring · folder line shown · Open enabled and enters configs",
                    open().picked(Some("configs")).row_focus("configs"),
                ),
            ],
        ),
        (
            "fp-row-focus-first-up",
            "Open mode · first ↑ with no row focus selects .env, the last row, with the ring · File name reads .env",
            vec![("", open().picked(Some(".env")).row_focus(".env"))],
        ),
        (
            "fp-row-focus-pointer",
            "Pointer over row focus · save mode on deploy.sh · open mode on README.md",
            vec![
                (
                    "Save mode · pointer over deploy.sh · hover + focus",
                    save().row_focus("deploy.sh").row_hover("deploy.sh"),
                ),
                (
                    "Open mode · pointer over selected README.md · selected + hover + focus",
                    open()
                        .picked(Some("README.md"))
                        .row_focus("README.md")
                        .row_hover("README.md"),
                ),
            ],
        ),
        (
            "fp-row-focus-after-click",
            "Open mode · click README.md · then Space",
            vec![
                (
                    "Click README.md · selected, no ring",
                    open().picked(Some("README.md")),
                ),
                (
                    "Then Space · ring on README.md · nothing else changes",
                    open().picked(Some("README.md")).row_focus("README.md"),
                ),
            ],
        ),
        (
            "fp-row-focus-save-space-folder",
            "Save mode · Space selects configs with the ring · folder line shown · File name keeps keybindings-2026-09-14.toml · Save",
            vec![("", save().picked(Some("configs")).row_focus("configs"))],
        ),
        (
            "fp-row-focus-folder-skip",
            "Folder mode · End moves row focus and selection to node_modules, the last folder · inert files after it skipped",
            vec![(
                "",
                folder()
                    .picked(Some("node_modules"))
                    .row_focus("node_modules"),
            )],
        ),
        (
            "fp-row-focus-folder-clear",
            "Folder mode · after a click on the empty part of the list · no selection, row focus or ring · Folder readout shows the current folder · hint shows the current-folder line",
            vec![("", folder().picked(None))],
        ),
    ]
}

/// 행 상태 여덟 칸 — 디자인 `FP_STATE_ROWS`: (이름, 선택, hover, 포커스).
pub(super) const ROW_STATES: [(&str, bool, bool, bool); 8] = [
    ("rest", false, false, false),
    ("hover", false, true, false),
    ("focus", false, false, true),
    ("hover + focus", false, true, true),
    ("selected", true, false, false),
    ("selected + focus", true, false, true),
    ("selected + hover", true, true, false),
    ("selected + hover + focus", true, true, true),
];

/// 상태 칸의 두 행 — 디자인 `FpRowStatesG` 의 configs · README.md.
fn state_rows() -> [&'static Row; 2] {
    let pick = |name| {
        FILES
            .iter()
            .find(|f| f.name == name)
            .expect("디자인 seed 행")
    };
    [pick("configs"), pick("README.md")]
}

fn caption(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .size(theme.font_size_caption.value())
            .color(theme.text_muted().to_egui()),
    );
}

/// 설명 한 줄 아래 카드 하나 — 디자인 `g-cap` + `FilePickerFrame`.
///
/// 폭은 카드 폭(`v.w` + 좌우 테두리)으로 첫 프레임부터 요청한다. 크기를 모르는 묶음은 첫 프레임에 줄을 바꾸지
/// 못하고, 그 프레임에 잰 넓은 폭을 감싸는 `spec::cluster` 가 다음 프레임에도 그대로 잡기 때문이다.
fn captioned(ui: &mut egui::Ui, theme: &Theme, (cap, v): Captioned) {
    let w = v.w.value() + 2.0 * theme.border_width.value();
    ui.allocate_ui_with_layout(
        egui::vec2(w, 0.0),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            ui.set_width(w);
            ui.spacing_mut().item_spacing.y = theme.spacing_xs.value();
            if !cap.is_empty() {
                caption(ui, theme, cap);
            }
            card(ui, theme, v);
        },
    );
}

pub fn draw_row_focus(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        for c in stage_cards() {
            captioned(ui, theme, c);
        }
    });
    row_states_view(ui, theme);
    for (name, label, cards) in views() {
        spec::column_cluster(ui, theme, &format!("View · {name} — {label}"), |ui| {
            for c in cards {
                captioned(ui, theme, c);
            }
        });
    }
    spec::meta(
        ui,
        theme,
        &[
            (
                "row focus",
                "keyboard's place in the list · row keys are ↑ ↓, Home, End, PageUp, PageDown, Space and → · a click on a row, Space or any key that selects a row moves row focus to that row",
            ),
            (
                "start",
                "with no row focus, ↓, PageDown and Home take the first row · ↑, PageUp and End take the last row · inert rows skipped",
            ),
            (
                "selection",
                "in open and folder mode, every key that moves row focus selects the same row · in save mode, movement keys move row focus only",
            ),
            (
                "ring",
                "explorer's current-item shape · 1px inset ring (fp-row-focus-ring-width), radius-sm corners · painted after the row, over the selection fill and bar · no layout shift",
            ),
            (
                "ink",
                "focus-ring cell of each row ground · fp-row-focus-ring (border-focus) at rest and under hover · fp-row-focus-ring-selected (accent-primary-raised) when selected · fp-row-focus-ring-selected-hover (accent-primary-on-active) when selected under the pointer",
            ),
            (
                "hover",
                "overlay-hover over the row · name text-secondary-raised · size and date text-muted · file glyph text-secondary · folder glyph accent-primary · selected under hover uses text-on-active-hover for name, size and date, text-on-active for the file glyph and accent-primary-on-active for the folder glyph",
            ),
            (
                "selection bar",
                "accent-primary-on-active on every selected row · painted over the fill and hover wash",
            ),
            (
                "shows",
                "a row key shows the ring if row focus exists afterward · a key that navigates clears row focus and the ring · a click in the list hides the ring · no ring when the picker opens · hidden while the picker lacks key focus",
            ),
        ],
        &[
            TokenChip::new(
                "fp-row-focus-ring",
                "ring · rest and hover",
                theme.fp_row_focus_ring().to_egui(),
            ),
            TokenChip::new(
                "fp-row-focus-ring-selected",
                "ring · selected",
                theme.fp_row_focus_ring_selected().to_egui(),
            ),
            TokenChip::new(
                "fp-row-focus-ring-selected-hover",
                "ring · selected under hover",
                theme.fp_row_focus_ring_selected_hover().to_egui(),
            ),
            TokenChip::new(
                "accent-primary-on-active",
                "selection bar",
                theme.accent_primary_on_active().to_egui(),
            ),
            TokenChip::without_color("fp-row-focus-ring-width", "ring line"),
        ],
    );
}

/// View `fp-row-focus-row-states` — 여덟 행 상태를 폴더 행 · 파일 행 상자로, Mocha · Latte 바탕마다 그린다.
fn row_states_view(ui: &mut egui::Ui, theme: &Theme) {
    let with_zoom =
        |base: Theme| Theme::with_colors_and_zoom(base.to_colors(), base.is_light, theme.ui_zoom);
    let grounds = [
        ("Mocha", with_zoom(tasty_themes::mocha_fallback())),
        ("Latte", with_zoom(crate::host_shell::latte_theme())),
    ];
    spec::column_cluster(
        ui,
        theme,
        "View · fp-row-focus-row-states — Eight row states · each shown as a folder row and a file row in separate boxes · Mocha and Latte",
        |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing =
                    egui::vec2(theme.spacing_lg.value(), theme.spacing_lg.value());
                for (label, th) in &grounds {
                    spec::wrap_item(ui, |ui| row_states_ground(ui, th, label));
                }
            });
        },
    );
}

/// 디자인 `GroundThemes` 의 한 바탕: 테마 이름과 여덟 행 상태 격자.
fn row_states_ground(ui: &mut egui::Ui, th: &Theme, label: &str) {
    egui::Frame::new()
        .fill(th.bg_app().to_egui())
        .stroke(egui::Stroke::new(
            th.border_width.value(),
            th.border_default().to_egui(),
        ))
        .corner_radius(th.corner_radius.value())
        .inner_margin(egui::Margin::same(th.spacing_md.value() as i8))
        .show(ui, |ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = th.spacing_md.value();
                ui.label(
                    egui::RichText::new(label)
                        .font(egui::FontId::monospace(th.font_size_micro.value()))
                        .color(th.text_muted().to_egui()),
                );
                egui::Grid::new(("fp-row-focus-row-states", label))
                    .num_columns(2)
                    .spacing(egui::vec2(th.spacing_md.value(), th.spacing_sm.value()))
                    .show(ui, |ui| {
                        for (name, selected, hover, focus) in ROW_STATES {
                            caption(ui, th, name);
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = th.spacing_xs.value();
                                for r in state_rows() {
                                    row_box(ui, th, r, selected, hover, focus);
                                }
                            });
                            ui.end_row();
                        }
                    });
            });
        });
}

/// 행 하나를 `bg-panel` · `border-frame` 상자에 담는다 — 디자인 상태 칸의 상자.
fn row_box(ui: &mut egui::Ui, th: &Theme, r: &Row, selected: bool, hover: bool, focus: bool) {
    egui::Frame::new()
        .fill(th.bg_panel().to_egui())
        .stroke(egui::Stroke::new(
            th.border_width.value(),
            th.border_frame().to_egui(),
        ))
        .corner_radius(th.corner_radius.value())
        .inner_margin(egui::Margin::same(th.border_width.value() as i8))
        .show(ui, |ui| {
            ui.set_width(th.fp_specimen_view_width().value());
            ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
            row(
                ui,
                th,
                r,
                RowState {
                    multi: false,
                    checked: false,
                    selected,
                    focus,
                    hover,
                    inert: false,
                    file_tooltip: "",
                },
            );
        });
}
