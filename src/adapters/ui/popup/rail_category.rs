//! 축소 레일 카테고리 팝업 (`sidebar_context_menu.jsx` `RailCategoryPopup` 전사).
//!
//! 레일의 `---` 카테고리 버튼을 누르면 버튼 **우측**에 앵커드로 뜬다(Tools 버튼과 동일
//! 앵커 패턴). 맨 위는 **클릭 불가한 카테고리 이름 헤더**(라벨만, count 없음), 그 아래 액션:
//! `Add workspace`(해당 카테고리 소속 생성) · `Collapse/Expand`(접힘 토글). 비-normal
//! 카테고리는 separator + `Rename`(→ rename 다이얼로그) / `Delete`(danger, → 삭제 confirm).
//!
//! `state.dialogs.rail_category_popup` 가 대상 카테고리 id 를 들고 있다. 없으면 즉시 닫힘.

use crate::adapters::ui::category_actions;
use crate::adapters::ui::icons;
use crate::adapters::ui::popup::{self, PopupAction};
use crate::i18n::t;
use crate::intent::Intent;
use crate::state::AppState;
use crate::theme;
use tasty_type_geometry::length::LogicalPx;

pub const RAIL_CATEGORY_POPUP_ID: &str = "rail_category";

/// 팝업 폭 (디자인 minWidth 176).
const POPUP_WIDTH: LogicalPx = LogicalPx(176.0);
/// 헤더(이름) 영역 높이 — 라벨 + 상하 패딩 + 하단 보더.
const HEADER_HEIGHT: LogicalPx = LogicalPx(30.0);

/// 현재 대상 카테고리가 접혀 있는지 등 팝업 렌더에 필요한 스냅샷.
struct Target {
    label: String,
    collapsed: bool,
    /// normal(예약) 여부 — true 면 Rename/Delete 를 노출하지 않는다(additive-only).
    is_reserved: bool,
}

/// `state.dialogs.rail_category_popup` 의 대상 카테고리를 engine 에서 해석.
fn resolve_target(state: &AppState, engine: &crate::core::CoreState) -> Option<Target> {
    let cat_id = state.dialogs.rail_category_popup?;
    let cat = engine.categories().iter().find(|c| c.id == cat_id)?;
    let label = if cat.is_normal() {
        t("sidebar.workspaces_heading").to_string()
    } else {
        cat.name.clone()
    };
    Some(Target {
        label,
        collapsed: cat.collapsed,
        is_reserved: cat.is_normal(),
    })
}

/// 아이콘 + 라벨 메뉴 행 1개. hover 시 overlay-hover 배경. `danger` 면 라벨/아이콘을
/// accent-danger 로 그린다(삭제). 클릭 여부 반환.
fn menu_row(
    ui: &mut egui::Ui,
    th: &crate::theme::Theme,
    icon: icons::Icon,
    label: &str,
    danger: bool,
) -> bool {
    let width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(
        egui::vec2(width, th.item_height_interactive.value()),
        egui::Sense::click(),
    );
    let radius = th.corner_radius.value();
    if resp.hovered() {
        ui.painter()
            .rect_filled(rect, radius, th.hover_overlay.to_egui_premultiplied());
    }
    let color: egui::Color32 = if danger {
        th.accent_danger().into()
    } else if resp.hovered() {
        th.text_primary().into()
    } else {
        th.text_muted().into()
    };
    let icon_size = th.icon_glyph_size_md.value();
    let icon_rect = egui::Rect::from_min_size(
        egui::pos2(
            rect.min.x + th.spacing_sm.value(),
            rect.center().y - icon_size / 2.0,
        ),
        egui::vec2(icon_size, icon_size),
    );
    icon.image(icon_size, color).paint_at(ui, icon_rect);
    ui.painter().text(
        egui::pos2(icon_rect.max.x + th.spacing_sm.value(), rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(th.font_size_body.value()),
        color,
    );
    resp.clicked()
}

/// PopupDef::on_close entry point — 어떤 경로로 닫히든 대상 카테고리 참조를 비운다.
pub fn on_close_rail_category_popup(
    _ctx: &egui::Context,
    state: &mut AppState,
    _engine: &mut crate::core::CoreState,
) {
    state.dialogs.rail_category_popup = None;
}

pub fn draw_rail_category_popup(
    ui: &mut egui::Ui,
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
) -> PopupAction {
    if ui.ctx().input(|i| i.key_pressed(egui::Key::Escape)) {
        return PopupAction::Close;
    }
    let Some(target) = resolve_target(state, engine) else {
        return PopupAction::Close;
    };
    let cat_id = state.dialogs.rail_category_popup.expect("resolved above");
    let th = theme::theme();

    // ── 비클릭 카테고리 이름 헤더 (라벨만 + 하단 보더 — count 표기 없음). ──
    let width = ui.available_width();
    let (header_rect, _) = ui.allocate_exact_size(
        egui::vec2(width, HEADER_HEIGHT.value()),
        egui::Sense::hover(),
    );
    ui.painter().text(
        egui::pos2(
            header_rect.min.x + th.spacing_sm.value(),
            header_rect.center().y,
        ),
        egui::Align2::LEFT_CENTER,
        &target.label,
        egui::FontId::proportional(th.font_size_body.value()),
        th.text_primary().into(),
    );
    // 하단 1px 보더 (separator).
    let border = egui::Rect::from_min_size(
        egui::pos2(
            header_rect.min.x,
            header_rect.max.y - th.border_width.value(),
        ),
        egui::vec2(width, th.border_width.value()),
    );
    ui.painter()
        .rect_filled(border, 0.0, th.separator.to_egui_premultiplied());
    ui.add_space(th.spacing_xs.value());

    // ── Add workspace — 이 카테고리 소속으로 새 워크스페이스 생성. ──
    if menu_row(
        ui,
        &th,
        icons::PLUS,
        t("workspace_category.add_workspace"),
        false,
    ) {
        state.dispatch_intent(
            Intent::NewWorkspace {
                kind: None,
                params: serde_json::Value::Null,
                category: Some(cat_id),
            }
            .from_user_menu("rail_category/add_workspace"),
        );
        return PopupAction::Close;
    }

    // ── Collapse / Expand — 접힘 토글 + 영속. ──
    let (collapse_icon, collapse_label) = if target.collapsed {
        (icons::CHEVRON_RIGHT, t("workspace_category.expand"))
    } else {
        (icons::CHEVRON_DOWN, t("workspace_category.collapse"))
    };
    if menu_row(ui, &th, collapse_icon, collapse_label, false) {
        engine.toggle_category_collapsed(cat_id);
        engine.mark_layout_dirty();
        return PopupAction::Close;
    }

    // ── 비-normal 카테고리: separator + Rename / Delete(danger). ──
    if !target.is_reserved {
        // 1px separator 라인.
        let width = ui.available_width();
        ui.add_space(th.spacing_xs.value());
        let (sep_rect, _) = ui.allocate_exact_size(
            egui::vec2(width, th.border_width.value()),
            egui::Sense::hover(),
        );
        ui.painter()
            .rect_filled(sep_rect, 0.0, th.separator.to_egui_premultiplied());
        ui.add_space(th.spacing_xs.value());

        if menu_row(
            ui,
            &th,
            icons::EDIT,
            t("workspace_category.rename_category"),
            false,
        ) {
            category_actions::open_rename_category_dialog(state, engine, cat_id);
            return PopupAction::Close;
        }
        if menu_row(
            ui,
            &th,
            icons::TRASH,
            t("workspace_category.delete_category"),
            true,
        ) {
            category_actions::open_delete_category_confirm(state, cat_id);
            return PopupAction::Close;
        }
    }

    PopupAction::None
}

/// PopupDef.sizer — 헤더 + 행 수로 height 계산. normal 은 Add/Collapse 2행, 비-normal 은
/// separator + Rename/Delete 를 더한 4행.
pub fn rail_category_sizer(state: &AppState, engine: &crate::core::CoreState) -> egui::Vec2 {
    let th = theme::theme();
    let reserved = resolve_target(state, engine)
        .map(|t| t.is_reserved)
        .unwrap_or(true);
    let rows = if reserved { 2u32 } else { 4u32 };
    let mut content_h = HEADER_HEIGHT
        + th.spacing_xs
        + th.item_height_interactive.scaled(rows as f32)
        + th.spacing_xs.scaled((rows.saturating_sub(1)) as f32);
    if !reserved {
        // separator(border_width) + 상하 spacing_xs.
        content_h += th.border_width + th.spacing_xs.scaled(2.0);
    }
    // `+ 1` 은 디자인 값이 아니라 반올림 안전 여유다(`popup/convert.rs` 의
    // `safety_margin` 과 같은 것). 값이 우연히 `size-1` 과 같을 뿐이라 토큰으로
    // 바꾸지 않는다 — 바꾸면 이름이 뜻을 속인다.
    egui::vec2(
        POPUP_WIDTH.value(),
        (popup::content_margin().scaled(2.0) + content_h + LogicalPx(1.0)).value(),
    )
}

/// PopupDef.default_size — register 시점 placeholder. sizer 가 매 프레임 재계산.
pub fn rail_category_default_size() -> egui::Vec2 {
    let th = theme::theme();
    egui::vec2(
        POPUP_WIDTH.value(),
        (popup::content_margin().scaled(2.0)
            + HEADER_HEIGHT
            + th.item_height_interactive.scaled(2.0))
        .value(),
    )
}
