//! 카테고리 삭제 destructive 확인 다이얼로그 (`overlays-shared.jsx` `CategoryDeleteFrame`
//! 전사). 즉시 삭제하지 않고 취소/삭제 2버튼 confirm 을 한 번 거친다. 삭제 버튼은
//! danger(`accent_danger` + `text_on_accent`), 헤더에 trash danger 글리프. 본문은 안전한
//! 결과(워크스페이스는 삭제되지 않고 normal 로 이동)를 안내한다.
//!
//! `state.dialogs.pending_category_delete` 가 대상 카테고리 id 를 들고 있다. 없거나
//! normal(방어) 이면 즉시 닫힘. 확인 시 `delete_category`(워크스페이스를 순서 보존하며
//! normal 로 귀속) + `mark_layout_dirty`.

use crate::adapters::ui::icons;
use crate::adapters::ui::popup::{self, PopupAction};
use crate::i18n::{t, t_fmt2};
use crate::state::AppState;
use crate::theme;
use tasty_type_geometry::length::LogicalPx;

pub const CONFIRM_DELETE_CATEGORY_POPUP_ID: &str = "confirm_delete_category";

/// 다이얼로그 폭 (디자인 380px).
const WIDTH: LogicalPx = LogicalPx(380.0);

/// 대상 카테고리 스냅샷(이름 + 소속 워크스페이스 수).
struct Target {
    name: String,
    count: usize,
}

/// `pending_category_delete` 의 대상 카테고리를 해석. 없거나 normal 이면 None(닫힘).
fn resolve_target(state: &AppState, engine: &crate::core::CoreState) -> Option<Target> {
    let cat_id = state.dialogs.pending_category_delete?;
    let cat = engine.categories().iter().find(|c| c.id == cat_id)?;
    if cat.is_normal() {
        return None;
    }
    Some(Target {
        name: cat.name.clone(),
        count: engine.workspaces_in_category(cat_id).len(),
    })
}

/// PopupDef.title_fn — headless 라 실제 타이틀바는 없지만, 접근성/디버그용 라벨.
pub fn confirm_delete_category_title(
    _state: &AppState,
    _engine: &crate::core::CoreState,
) -> String {
    t("workspace_category.delete_confirm_title").to_string()
}

/// PopupDef.sizer — 본문 길이에 따라 height 조정(소형 모달).
pub fn confirm_delete_category_sizer(
    state: &AppState,
    engine: &crate::core::CoreState,
) -> egui::Vec2 {
    let body_len = resolve_target(state, engine)
        .map(|tgt| tgt.name.chars().count() + 60)
        .unwrap_or(60);
    let approx_lines = (body_len as f32 / 42.0).ceil().max(2.0);
    let body_h = approx_lines * theme::theme().font_size_body.value() * 1.5;
    // 헤더(글리프+제목) + 본문 + 버튼 행 + 여백.
    let content_h = 24.0 + body_h + 40.0;
    egui::vec2(
        WIDTH.value(),
        (popup::content_margin().scaled(2.0) + LogicalPx(content_h)).value(),
    )
}

/// PopupDef::on_close entry point — 어떤 경로로 닫히든(취소/외부/Escape) 삭제 대상을 비운다.
pub fn on_close_confirm_delete_category(
    _ctx: &egui::Context,
    state: &mut AppState,
    _engine: &mut crate::core::CoreState,
) {
    state.dialogs.pending_category_delete = None;
}

pub fn draw_confirm_delete_category(
    ui: &mut egui::Ui,
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
) -> PopupAction {
    let ctx = ui.ctx().clone();
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        state.dialogs.pending_category_delete = None;
        return PopupAction::Close;
    }
    let Some(target) = resolve_target(state, engine) else {
        state.dialogs.pending_category_delete = None;
        return PopupAction::Close;
    };
    let cat_id = state
        .dialogs
        .pending_category_delete
        .expect("resolved above");
    let th = theme::theme();

    let margin = th.spacing_sm.value();
    let available = ui.available_rect_before_wrap();
    let inner_rect = available.shrink2(egui::vec2(margin, th.spacing_xs.value()));
    let mut child_ui = ui.new_child(egui::UiBuilder::new().max_rect(inner_rect));
    let ui = &mut child_ui;

    // ── 헤더: trash danger 글리프 + "카테고리를 삭제할까요?" (semibold). ──
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = th.spacing_sm.value();
        let icon_size = th.icon_glyph_size_md.value();
        let (icon_rect, _) =
            ui.allocate_exact_size(egui::vec2(icon_size, icon_size), egui::Sense::hover());
        icons::TRASH
            .image(icon_size, th.accent_danger().into())
            .paint_at(ui, icon_rect);
        ui.label(
            egui::RichText::new(t("workspace_category.delete_confirm_title"))
                .color(th.text_primary())
                .size(th.font_size_body.value())
                .strong(),
        );
    });

    ui.add_space(th.spacing_sm.value());

    // ── 본문: 안전한 결과 안내(이름 + 워크스페이스 수 보간). ──
    ui.label(
        egui::RichText::new(t_fmt2(
            "workspace_category.delete_confirm_body",
            &target.name,
            &target.count.to_string(),
        ))
        .color(th.text_secondary())
        .size(th.font_size_body.value()),
    );

    // ── 푸터: 우측 정렬 Cancel(ghost) + Delete category(danger). ──
    let mut confirm = false;
    let mut cancel = false;
    ui.with_layout(egui::Layout::bottom_up(egui::Align::RIGHT), |ui| {
        ui.add_space(th.spacing_sm.value());
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Delete category — danger 채움 버튼.
                let del = egui::Button::new(
                    egui::RichText::new(t("workspace_category.delete_category"))
                        .color(th.text_on_accent()),
                )
                .fill(th.accent_danger());
                if ui.add(del).clicked() {
                    confirm = true;
                }
                if ui.button(t("button.cancel")).clicked() {
                    cancel = true;
                }
            });
        });
    });

    if cancel {
        state.dialogs.pending_category_delete = None;
        return PopupAction::Close;
    }
    if confirm {
        if let Err(e) = engine.delete_category(cat_id) {
            tracing::warn!("delete_category {cat_id} failed: {e:?}");
        }
        engine.mark_layout_dirty();
        state.dialogs.pending_category_delete = None;
        return PopupAction::Close;
    }
    PopupAction::None
}
