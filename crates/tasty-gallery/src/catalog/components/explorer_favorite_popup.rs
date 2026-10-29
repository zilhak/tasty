//! 즐겨찾기 추가 팝업 예제. 본체는 rename 팝업의 ExplorerAddFavorite 대상으로 열며
//! 윈도우 중앙에 표시한다. 콘텐츠는 본체와 같은 공용 rename view(탐색기 치수)다.

use tasty_type_appearance::theme::Theme;

use crate::catalog::spec::{self, StageVariant, TokenChip};
use crate::i18n::t;

/// 즐겨찾기 추가 팝업 카드 한 장.
pub(super) fn card(ui: &mut egui::Ui, theme: &Theme) {
    super::rename_popup::card(
        ui,
        theme,
        super::rename_popup::Card {
            layout: tasty_ui_widgets::RenamePopupLayout::Explorer,
            title: t("explorer.popup.add_favorite.title"),
            save: t("explorer.popup.add_favorite.add"),
            name: "Downloads",
            error: None,
            waiting: None,
        },
    );
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| card(ui, theme));

    spec::meta(
        ui,
        theme,
        &[
            ("frame", "360 × ui scale · dialog-bg · 1px border-strong"),
            ("title", "heading semibold"),
            ("input", "28 · input-bg · seeded folder name"),
            ("footer", "right — Cancel · Add (primary)"),
        ],
        &[
            TokenChip::new("dialog-bg", "frame", theme.dialog_bg().to_egui()),
            TokenChip::new("input-bg", "input", theme.input_bg().to_egui()),
            TokenChip::new(
                "accent-primary",
                "Add button",
                theme.accent_primary().to_egui(),
            ),
            TokenChip::without_color("shadow-modal", "popup lift"),
        ],
    );

    spec::note(
        ui,
        theme,
        "The host opens the shared Rename popup in window scope. Title, initial value and \
         primary button label differ. Favorites are global.",
    );
}
