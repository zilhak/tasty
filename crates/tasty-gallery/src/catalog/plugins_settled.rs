//! Markdown 플러그인의 결정 기록 Spec: 문서 바탕 하나, 주소 표시줄 상태와 대용량 확인.
//! 시안 `plugins.jsx`의 같은 Spec을 옮긴다.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{ControlSize, IconButton, Input};

use crate::catalog::components::md_large_file;
use crate::catalog::icons;
use crate::catalog::spec::{StageVariant, TokenChip, cluster, meta, note, stage};

/// 바탕 후보 견본 한 장의 크기.
const BED_W: LogicalPx = LogicalPx(148.0);
const BED_H: LogicalPx = LogicalPx(60.0);
/// 견본 안 표·코드 띠의 높이.
const BED_STRIPE_H: LogicalPx = LogicalPx(10.0);
/// 주소 표시줄 예제와 타일의 폭, 타일 높이.
const ADDRESS_W: LogicalPx = LogicalPx(440.0);
const TILE_H: LogicalPx = LogicalPx(232.0);
/// 타일 본문은 확인 팝업 뒤에서 흐리게 보인다.
const TILE_BODY_OPACITY: f32 = 0.35;

#[inline]
fn ec(c: impl Into<egui::Color32>) -> egui::Color32 {
    c.into()
}

/// 바탕 후보 하나. 표 줄무늬와 코드 칸을 얹어 단계가 구분되는지 본다.
fn bed(ui: &mut egui::Ui, theme: &Theme, label: &str, fill: egui::Color32, chosen: bool) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = theme.spacing_sm.value() * 0.75;
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(BED_W.value(), BED_H.value()),
            egui::Sense::hover(),
        );
        let p = ui.painter();
        let r = theme.corner_radius.value();
        p.rect_filled(rect, r, fill);
        let edge = if chosen {
            theme.accent_primary()
        } else {
            theme.border_default()
        };
        p.rect_stroke(
            rect,
            r,
            egui::Stroke::new(theme.border_width.value(), ec(edge)),
            egui::StrokeKind::Inside,
        );
        let inner = rect.shrink(theme.spacing_sm.value() * 0.75);
        let stripe = BED_STRIPE_H.value();
        let gap = theme.spacing_xs.value() * 0.75;
        let code =
            egui::Rect::from_min_max(egui::pos2(inner.left(), inner.bottom() - stripe), inner.max);
        let zebra = code.translate(egui::vec2(0.0, -(stripe + gap)));
        p.rect_filled(zebra, 0.0, ec(theme.md_table_row_bg_zebra()));
        p.rect_filled(code, 0.0, ec(theme.md_code_bg()));
        let ink = if chosen {
            theme.text_primary()
        } else {
            theme.text_muted()
        };
        ui.label(
            egui::RichText::new(label)
                .size(theme.font_size_caption.value())
                .color(ec(ink)),
        );
    });
}

/// Plugins › Markdown — 웹뷰 경로에는 포커스 신호가 없어 문서 바탕은 하나다.
pub fn draw_doc_background(ui: &mut egui::Ui, theme: &Theme) {
    let md = theme.surface("markdown");
    stage(ui, theme, StageVariant::Wrap, |ui| {
        // 후보 단계는 같은 primitive를 가리키는 semantic 접근자로 그린다:
        // crust = bg-app, mantle = bg-sidebar, base = bg-panel, #000 = 터미널 포커스 바탕.
        bed(ui, theme, "crust — chosen", ec(md.focused_bg), true);
        bed(
            ui,
            theme,
            "mantle — collides with zebra",
            ec(theme.bg_sidebar()),
            false,
        );
        bed(
            ui,
            theme,
            "base — flattens table fill",
            ec(theme.bg_panel()),
            false,
        );
        bed(
            ui,
            theme,
            "#000 — terminal only",
            ec(theme.surface("terminal").focused_bg),
            false,
        );
    });
    meta(
        ui,
        theme,
        &[
            ("backgrounds", "one (no focus swap on the webview path)"),
            ("mocha", "crust #11111b"),
            ("latte", "crust #dce0e8 (tracks bg-app, no branch)"),
            ("terminal", "unchanged — pure black stays terminal-only"),
            ("unfocused_bg", "unused by the webview renderer"),
        ],
        &[
            TokenChip::new("md-doc-bg", "document bed → --md-bg", ec(md.focused_bg)),
            TokenChip::new(
                "md-code-bg",
                "code fill → --md-code-bg",
                ec(theme.md_code_bg()),
            ),
            TokenChip::new(
                "md-table-row-bg-zebra",
                "zebra → --md-zebra",
                ec(theme.md_table_row_bg_zebra()),
            ),
            TokenChip::new(
                "md-table-border",
                "grid → --md-border",
                ec(theme.md_table_border()),
            ),
            TokenChip::new("md-quote-bar", "blockquote bar", ec(theme.md_quote_bar())),
            TokenChip::new(
                "md-rule",
                "horizontal rule",
                theme.md_rule().to_egui_premultiplied(),
            ),
        ],
    );
    note(
        ui,
        theme,
        "crust is the palette's deepest tone, so every content fill still steps up: crust (page) < mantle (table zebra) < base < surface0 (code + table header) < surface1 (table grid). mantle would swallow the zebra stripe; base would flatten the table's own fill against the page.",
    );
}

/// 주소 표시줄 한 줄. 편집 중이면 포커스 링을 함께 그린다(입력 포커스는 옮기지 않는다).
fn address_bar(ui: &mut egui::Ui, theme: &Theme, path: &str, editing: bool) {
    let gap = theme.spacing_sm.value();
    let go_side = ControlSize::Sm.height(theme);
    let field_w = (ADDRESS_W.value() - go_side - gap * 3.0).max(0.0);
    egui::Frame::new()
        .fill(ec(theme.bg_sidebar()))
        .inner_margin(egui::Margin::same(theme.spacing_sm.value() as i8))
        .show(ui, |ui| {
            ui.set_width(ADDRESS_W.value() - gap * 2.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                let mut buf = path.to_string();
                let file = |ui: &mut egui::Ui, rect: egui::Rect, c: egui::Color32| {
                    icons::FILE.image(rect.height(), c).paint_at(ui, rect);
                };
                let mut field = Input::new().mono(true).width(field_w).icon(&file);
                if !editing {
                    field = field.text_color(ec(theme.text_secondary()));
                }
                let resp = field.show(ui, theme, &mut buf);
                if editing {
                    ui.painter().rect_stroke(
                        resp.rect,
                        theme.corner_radius.value(),
                        egui::Stroke::new(
                            theme.focus_ring_width.value(),
                            ec(theme.input_border_focus()),
                        ),
                        egui::StrokeKind::Outside,
                    );
                }
                IconButton::new()
                    .size(ControlSize::Sm)
                    .show(ui, theme, &|ui, rect, c| {
                        icons::ARROW_RIGHT
                            .image(rect.height(), c)
                            .paint_at(ui, rect)
                    });
            });
        });
}

/// Plugins › Markdown — 주소 표시줄의 두 상태와 타일에 묶인 대용량 확인.
pub fn draw_address_states(ui: &mut egui::Ui, theme: &Theme) {
    stage(ui, theme, StageVariant::Column, |ui| {
        cluster(ui, theme, "display (idle)", |ui| {
            ui.push_id("idle", |ui| {
                address_bar(ui, theme, "~/work/tasty/README.md", false)
            });
        });
        cluster(ui, theme, "editing (focus)", |ui| {
            ui.push_id("editing", |ui| {
                address_bar(ui, theme, "~/work/tasty/docs/design/systems/theme.md", true)
            });
        });
        cluster(
            ui,
            theme,
            "over 1 MiB — confirm scoped to the tile",
            |ui| {
                let (tile, _) = ui.allocate_exact_size(
                    egui::vec2(ADDRESS_W.value(), TILE_H.value()),
                    egui::Sense::hover(),
                );
                let p = ui.painter();
                p.rect_filled(tile, theme.corner_radius.value(), ec(theme.bg_panel()));
                let mut child = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(tile)
                        .layout(egui::Layout::top_down(egui::Align::Min)),
                );
                child.set_clip_rect(tile.intersect(ui.clip_rect()));
                child.spacing_mut().item_spacing.y = 0.0;
                child.push_id("tile", |ui| {
                    address_bar(ui, theme, "~/work/tasty/docs/CHANGELOG-full.md", false)
                });
                let bar_bottom = child.min_rect().bottom();
                child.scope(|ui| {
                    ui.multiply_opacity(TILE_BODY_OPACITY);
                    egui::Frame::new()
                        .inner_margin(egui::Margin::symmetric(
                            theme.spacing_lg.value() as i8,
                            theme.spacing_md.value() as i8,
                        ))
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new("Changelog")
                                    .size(theme.font_size_prose_h1.value())
                                    .color(ec(theme.text_primary())),
                            );
                            ui.label(
                                egui::RichText::new("A very long history…")
                                    .size(theme.font_size_body.value())
                                    .color(ec(theme.text_secondary())),
                            );
                        });
                });
                // 막은 범위는 주소 표시줄 아래 타일뿐이다. 창 전체를 덮지 않는다.
                let scope = egui::Rect::from_min_max(egui::pos2(tile.left(), bar_bottom), tile.max);
                ui.painter()
                    .with_clip_rect(scope.intersect(ui.clip_rect()))
                    .rect_filled(scope, 0.0, ec(theme.scrim()));
                // 시안 scrim 은 `alignItems: center; justifyContent: center` 다. 카드 크기를 먼저
                // 재고 막은 범위의 정중앙에 놓는다.
                let inner = scope.shrink(theme.spacing_md.value());
                let mut sizing = ui.new_child(
                    egui::UiBuilder::new()
                        .id_salt("large-file-sizing")
                        .max_rect(inner)
                        .layout(egui::Layout::top_down(egui::Align::Min))
                        .sizing_pass()
                        .invisible(),
                );
                md_large_file::popup_card(&mut sizing, theme);
                let card = egui::Rect::from_center_size(inner.center(), sizing.min_rect().size());
                let mut popup = ui.new_child(
                    egui::UiBuilder::new()
                        .id_salt("large-file-popup")
                        .max_rect(card)
                        .layout(egui::Layout::top_down(egui::Align::Min)),
                );
                popup.set_clip_rect(scope.intersect(ui.clip_rect()));
                md_large_file::popup_card(&mut popup, theme);
            },
        );
    });
    meta(
        ui,
        theme,
        &[
            ("field", "mono path · click to edit"),
            ("idle", "secondary text, input-border"),
            ("editing", "focus ring + caret, input-border-focus"),
            ("Go", "IconButton · ↵ also confirms"),
            ("popup scope", "surface (clamped to tile)"),
            ("popup", "360px · Open / Cancel"),
        ],
        &[
            TokenChip::new("input-bg", "field fill", ec(theme.input_bg())),
            TokenChip::new(
                "input-border-focus",
                "edit border",
                ec(theme.input_border_focus()),
            ),
            TokenChip::new("border-focus", "focus ring", ec(theme.border_focus())),
            TokenChip::new("accent-warning", "size chip", ec(theme.accent_warning())),
            TokenChip::without_color("shadow-modal", "popup lift"),
            TokenChip::new(
                "accent-primary",
                "Open (primary)",
                ec(theme.accent_primary()),
            ),
        ],
    );
    note(
        ui,
        theme,
        "The address bar is the shared editable path field with a file leading icon — same edit / navigate / revert contract as the Explorer toolbar. The large-file confirm is surface-scoped, so it dims only this tile; Cancel opens nothing.",
    );
}
