//! `explorer_favorite_popup` specimen — 디자인 T11 "Add to favorites" 팝업
//! (design §3.5, 와이어프레임 8).
//!
//! 결정: Popup (`PopupDef`, modal 아님 — 경량 입력·앵커드). rename 팝업과 동일 골격
//! (title / path caption / input / Cancel·primary footer) — 타이틀·초기값·primary
//! 라벨만 다르다. 공유 frame 키트(`widgets::dialog`) 재사용.
//!
//! i18n 키 후보(본체): `explorer.popup.add_favorite.title/path/add`, `common.cancel`.

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{Button, ButtonVariant};

use crate::catalog::spec::{self, StageVariant, TokenChip};
use crate::catalog::widgets::dialog as kit;

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    let width = theme.measure_sm; // ≈300 (narrow column, design w≈280)
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        kit::frame_card(ui, theme, width, kit::panel_fill(theme), |ui| {
            kit::region_sym(ui, theme.spacing_md, theme.spacing_md, |ui| {
                ui.spacing_mut().item_spacing.y = theme.spacing_sm.value();
                kit::title(ui, theme, "Add to favorites");
                kit::caption(ui, theme, "Path: ~/Downloads", false);
                // 초기값 = 폴더명. gallery 는 정적 표시(focus 경합 회피).
                kit::field(ui, theme, None, "Downloads", false, false);
                ui.add_space(theme.spacing_xs.value());
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
                        Button::new("Add")
                            .variant(ButtonVariant::Primary)
                            .show(ui, theme);
                        Button::new("Cancel")
                            .variant(ButtonVariant::Ghost)
                            .show(ui, theme);
                    });
                });
            });
        });
    });

    spec::meta(
        ui,
        theme,
        &[
            ("frame", "≈280 · bg-panel · 1px border-strong"),
            ("title", "heading semibold"),
            ("path", "caption · text-muted"),
            ("input", "28 · surface-raised · seeded folder name"),
            ("footer", "right — Cancel · Add (primary)"),
        ],
        &[
            TokenChip::new("bg-panel", "frame", theme.bg_panel().to_egui()),
            TokenChip::new("surface-raised", "input", theme.surface_raised().to_egui()),
            TokenChip::new(
                "accent-primary",
                "Add button",
                theme.accent_primary().to_egui(),
            ),
        ],
    );

    spec::note(
        ui,
        theme,
        "Same skeleton as the Rename popup — only the title, seeded value, and primary \
         label differ. Anchored near the trigger (context menu / sidebar action); a \
         lightweight popup, not a scrim-blocking modal. Favorites are global.",
    );
}
