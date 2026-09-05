//! Workspace category overlays — 디자인(gallery) `CategoryEditFrame` / `CategoryDeleteFrame`
//! / `RailCategoryPopup` specimen.
//!
//! - **Edit dialog**: 360px 단일필드(Rename 다이얼로그 재사용) — 생성/이름변경. 검증 에러
//!   상태(예약어 normal)는 danger 라인 + 확인 비활성.
//! - **Delete confirm**: 380px destructive — trash danger 글리프 + 제목 + 안전 결과 본문 +
//!   Cancel/Delete(danger).
//! - **Rail popup**: 176px 앵커드 — 비클릭 이름 헤더(라벨만) + Add workspace/Collapse +
//!   (비-normal) Rename/Delete(danger).
//!
//! Theme 토큰만으로 정적 재현.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{Button, ButtonVariant};

use crate::catalog::icons::{CHEVRON_DOWN, EDIT, PLUS, TRASH};
use crate::catalog::spec::{self, StageVariant, TokenChip};
use crate::catalog::widgets::dialog as kit;

const EDIT_WIDTH: LogicalPx = LogicalPx(360.0);
const DELETE_WIDTH: LogicalPx = LogicalPx(380.0);
const POPUP_WIDTH: LogicalPx = LogicalPx(176.0);

/// 생성/이름변경 다이얼로그 — `error` 가 `Some` 이면 검증 에러 상태(확인 비활성).
fn edit_dialog(ui: &mut egui::Ui, theme: &Theme, value: &str, error: Option<&str>) {
    kit::frame_card(ui, theme, EDIT_WIDTH, kit::panel_fill(theme), |ui| {
        kit::region_sym(
            ui,
            theme.spacing_md.value(),
            theme.spacing_md.value(),
            |ui| {
                ui.spacing_mut().item_spacing.y = theme.spacing_sm.value();
                kit::title(ui, theme, "New category");
                kit::field(ui, theme, None, value, value.is_empty(), false);
                if let Some(err) = error {
                    ui.colored_label(theme.accent_danger().to_egui(), err);
                }
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let mut create = Button::new("Create").variant(ButtonVariant::Primary);
                        if error.is_some() {
                            create = create.enabled(false);
                        }
                        create.show(ui, theme);
                        Button::new("Cancel")
                            .variant(ButtonVariant::Ghost)
                            .show(ui, theme);
                    });
                });
            },
        );
    });
}

/// 삭제 destructive confirm.
fn delete_confirm(ui: &mut egui::Ui, theme: &Theme) {
    kit::frame_card(ui, theme, DELETE_WIDTH, kit::panel_fill(theme), |ui| {
        kit::region_sym(
            ui,
            theme.spacing_md.value(),
            theme.spacing_md.value(),
            |ui| {
                ui.spacing_mut().item_spacing.y = theme.spacing_sm.value();
                ui.horizontal(|ui| {
                    kit::icon(
                        ui,
                        TRASH,
                        theme.icon_glyph_size_md.value(),
                        theme.accent_danger().to_egui(),
                    );
                    kit::title(ui, theme, "Delete category?");
                });
                kit::body(
                    ui,
                    theme,
                    "Delete Services? Its 3 workspaces aren't deleted — they move back to Workspaces.",
                );
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        Button::new("Delete category")
                            .variant(ButtonVariant::Danger)
                            .show(ui, theme);
                        Button::new("Cancel")
                            .variant(ButtonVariant::Ghost)
                            .show(ui, theme);
                    });
                });
            },
        );
    });
}

/// 레일 카테고리 팝업 (비클릭 이름 헤더 + 액션 행). `danger` 행은 accent-danger.
fn rail_popup(ui: &mut egui::Ui, theme: &Theme) {
    kit::frame_card(ui, theme, POPUP_WIDTH, kit::raised_fill(theme), |ui| {
        kit::region_sym(
            ui,
            theme.spacing_sm.value(),
            theme.spacing_sm.value(),
            |ui| {
                // 비클릭 이름 헤더 (라벨만 — count 표기 없음).
                ui.label(
                    egui::RichText::new("Services")
                        .color(theme.text_primary().to_egui())
                        .size(theme.font_size_body.value())
                        .strong(),
                );
                kit::hsep(ui, theme);
                popup_row(ui, theme, PLUS, "Add workspace", false);
                popup_row(ui, theme, CHEVRON_DOWN, "Collapse", false);
                kit::hsep(ui, theme);
                popup_row(ui, theme, EDIT, "Rename category", false);
                popup_row(ui, theme, TRASH, "Delete category", true);
            },
        );
    });
}

/// 팝업 메뉴 행 1개 — 아이콘 + 라벨(28px). `danger` 면 accent-danger.
fn popup_row(
    ui: &mut egui::Ui,
    theme: &Theme,
    glyph: crate::catalog::icons::MockGlyph,
    label: &str,
    danger: bool,
) {
    let color = if danger {
        theme.accent_danger().to_egui()
    } else {
        theme.text_secondary().to_egui()
    };
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), theme.item_height_interactive.value()),
        egui::Sense::hover(),
    );
    let icon_size = theme.icon_glyph_size_md.value();
    let icon_c = egui::pos2(
        rect.min.x + theme.spacing_sm.value() + icon_size * 0.5,
        rect.center().y,
    );
    glyph.image(icon_size, color).paint_at(
        ui,
        egui::Rect::from_center_size(icon_c, egui::vec2(icon_size, icon_size)),
    );
    ui.painter().text(
        egui::pos2(
            icon_c.x + icon_size * 0.5 + theme.spacing_sm.value(),
            rect.center().y,
        ),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(theme.font_size_body.value()),
        color,
    );
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        spec::cluster(ui, theme, "Create / rename", |ui| {
            edit_dialog(ui, theme, "Services", None)
        });
        spec::cluster(ui, theme, "Validation error", |ui| {
            edit_dialog(
                ui,
                theme,
                "normal",
                Some("'normal' is a reserved category name."),
            )
        });
        spec::cluster(ui, theme, "Delete confirm", |ui| delete_confirm(ui, theme));
        spec::cluster(ui, theme, "Rail popup", |ui| rail_popup(ui, theme));
    });

    spec::meta(
        ui,
        theme,
        &[
            ("edit", "360px · single field + inline validation"),
            ("delete", "380px · destructive · trash glyph"),
            ("rail popup", "176px · name header + actions"),
            (
                "error",
                "reserved / duplicate / empty → danger line + disabled",
            ),
        ],
        &[
            TokenChip::new("bg-panel", "edit/delete frame", theme.bg_panel().to_egui()),
            TokenChip::new(
                "surface-raised",
                "rail popup frame",
                theme.surface_raised().to_egui(),
            ),
            TokenChip::new(
                "accent-danger",
                "delete + error",
                theme.accent_danger().to_egui(),
            ),
        ],
    );

    spec::note(
        ui,
        theme,
        "생성/이름변경은 360px 단일필드 다이얼로그를 재사용하고 확인 시 백엔드와 동일 규칙 \
         (빈/예약어 normal/중복) 으로 라이브 검증한다. 삭제는 destructive confirm 을 한 번 \
         거치며 본문이 안전한 결과(워크스페이스는 normal 로 이동)를 안내한다. 레일 팝업은 \
         `---` 버튼 우측에 앵커드로 뜬다.",
    );
}
