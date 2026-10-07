//! 파일 선택 footer 의 확장자 필터 칩 — 읽기 전용 표시. 디자인 Spec "File-type filter chip".

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;

use super::{FpState, SaveState, Variant, card};
use crate::catalog::spec::{self, StageVariant, TokenChip};

/// 디자인 Spec 의 카드 폭(`FilePickerFrame w={480}`). 칩 갈래를 나란히 놓으려고 줄인 무대 치수다.
const CHIP_FRAME_W: LogicalPx = LogicalPx(480.0);
/// 디자인 Spec 의 카드 높이(`FilePickerFrame h={300}`, `--tasty-size-300`). 대응 역할 토큰이 없다.
const CHIP_FRAME_H: LogicalPx = LogicalPx(300.0);

pub fn draw_filter_chip(ui: &mut egui::Ui, theme: &Theme) {
    let open = Variant::open(FpState::Loaded, false, false).sized(CHIP_FRAME_W, CHIP_FRAME_H);
    let save = Variant::save(SaveState::New, false).sized(CHIP_FRAME_W, CHIP_FRAME_H);
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        spec::cluster(ui, theme, "no filter — chip hidden", |ui| {
            card(ui, theme, open);
        });
        spec::cluster(ui, theme, "one extension", |ui| {
            card(ui, theme, open.filtered(&["toml"]));
        });
        spec::cluster(ui, theme, "two extensions — save mode", |ui| {
            card(ui, theme, save.filtered(&["toml", "json"]));
        });
        spec::cluster(
            ui,
            theme,
            "many — capped at 160, ellipsis + tooltip",
            |ui| {
                card(
                    ui,
                    theme,
                    open.filtered(&["png", "jpg", "jpeg", "gif", "webp", "svg"]),
                );
            },
        );
    });
    spec::meta(
        ui,
        theme,
        &[
            (
                "role",
                "read-only readout — not a button, no menu, not in tab order",
            ),
            (
                "label",
                "*.ext list, mono caption, caller order, joined by \", \"",
            ),
            ("no filter", "chip absent"),
            (
                "width",
                "content, ≤ fp-filter-max-width (160) · then end ellipsis",
            ),
            ("tooltip", "Showing *.png, *.jpg, … (full list)"),
            ("height", "fp-filter-height = the Input beside it"),
            ("box", "1px separator · radius · no fill · pad-x space-sm"),
            ("save mode", "no extension auto-append"),
            (
                "i18n",
                "only the tooltip prefix is translated; the list is literal",
            ),
        ],
        &[
            TokenChip::without_color("fp-filter-height", "28 — matches the Input"),
            TokenChip::without_color("fp-filter-max-width", "160 cap (new)"),
            TokenChip::new(
                "separator",
                "chip edge",
                theme.separator.to_egui_premultiplied(),
            ),
            TokenChip::new("text-muted", "list", theme.text_muted().to_egui()),
        ],
    );
    spec::dont(
        ui,
        theme,
        "Don't draw it with a chevron or a hover fill while nothing opens — the earlier static \
         \"All files ▾\" chip promised a menu that does not exist.",
    );
}
