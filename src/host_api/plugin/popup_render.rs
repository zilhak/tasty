//! Plugin popup 인스턴스 렌더링.
//!
//! `PluginManager::popup_instances`에 등록된 popup을 매 프레임 egui::Area로 그린다.
//! 호스트 본문 popup(`PopupManager`)과는 별도 경로 — plugin popup은 동적 instance_id를
//! 가지고 `&'static str` 기반 `PopupId`/`PopupDef` 모델에 맞지 않기 때문.
//!
//! 사용자 입력은 [`super::ui_tree_render::PopupSink`]를 통해 모은 뒤 `state` 큐에 적재해
//! 메인 루프가 `PluginManager::send_popup_event`로 forward한다.

use egui::{Context, Id, Order, Stroke, Vec2};
use tasty_plugin_protocol::PopupCloseReason;

use super::PluginManager;
use super::manifest::PopupAnchor;
use super::ui_tree_render::{PopupSink, render_popup_tree};
use crate::gpu::canvas_texture::CanvasTextureCache;
use crate::state::AppState;
use crate::ui::popup::CONTENT_MARGIN;

const DEFAULT_POPUP_SIZE: Vec2 = Vec2::new(360.0, 200.0);

/// 매 egui 프레임 호출. plugin popup_instances를 순회하면서:
///  - tree가 있는 인스턴스를 egui::Area로 렌더,
///  - PopupSink로 수집한 UiEvent를 `state.plugin_popup_events`에 적재,
///  - 외부 클릭/Escape를 감지해 `state.plugin_popup_closes`에 적재.
///
/// `mgr`이 `None`이거나 popup_instances가 비어있으면 즉시 반환.
pub fn draw_plugin_popups(
    ctx: &Context,
    state: &mut AppState,
    plugin_manager: Option<&PluginManager>,
    canvas_cache: &CanvasTextureCache,
) {
    let Some(mgr) = plugin_manager else {
        return;
    };

    // popup_instances를 즉시 owned snapshot으로 복사. 이후 loop에서 `state`를 mutable로
    // borrow하기 위함.
    struct Snap {
        instance_id: u64,
        plugin_id: String,
        tree: tasty_plugin_protocol::ui_tree::UiNode,
        anchor: PopupAnchor,
        size: Vec2,
        dismiss_on_outside_click: bool,
    }
    let snaps: Vec<Snap> = mgr
        .popup_instances()
        .filter_map(|(id, inst)| {
            let tree = inst.tree.clone()?;
            let size = inst
                .contribute
                .size_hint
                .map(|s| Vec2::new(s.width as f32, s.height as f32))
                .unwrap_or(DEFAULT_POPUP_SIZE);
            Some(Snap {
                instance_id: id,
                plugin_id: inst.plugin_id.clone(),
                tree,
                anchor: inst.contribute.anchor,
                size,
                dismiss_on_outside_click: inst.contribute.dismiss_on_outside_click,
            })
        })
        .collect();

    if snaps.is_empty() {
        return;
    }

    let screen_rect = ctx.screen_rect();
    let pointer_pos = ctx.input(|i| i.pointer.hover_pos());
    let primary_pressed = ctx.input(|i| i.pointer.primary_pressed());
    let escape_pressed = ctx.input(|i| i.key_pressed(egui::Key::Escape));

    let mut any_hovered = false;

    for snap in snaps {
        let pos = anchor_pos(snap.anchor, snap.size, screen_rect, pointer_pos);
        // 화면 경계 내로 clamp.
        let pos = egui::pos2(
            pos.x.clamp(
                screen_rect.min.x,
                (screen_rect.max.x - snap.size.x).max(screen_rect.min.x),
            ),
            pos.y.clamp(
                screen_rect.min.y,
                (screen_rect.max.y - snap.size.y).max(screen_rect.min.y),
            ),
        );
        let rect = egui::Rect::from_min_size(pos, snap.size);

        if let Some(p) = pointer_pos
            && rect.contains(p)
        {
            any_hovered = true;
        }

        let area_id = Id::new("plugin_popup").with(snap.instance_id);
        let layer_id = egui::LayerId::new(Order::Foreground, area_id);
        let painter = ctx.layer_painter(layer_id);
        let th = crate::theme::theme();
        painter.rect_filled(rect, th.corner_radius.value(), th.surface0);
        painter.rect_stroke(
            rect,
            th.corner_radius.value(),
            Stroke::new(th.border_width.value(), th.surface1),
            egui::StrokeKind::Outside,
        );

        let content_rect = rect.shrink(CONTENT_MARGIN);
        let mut child_ui = egui::Ui::new(
            ctx.clone(),
            Id::new("plugin_popup_content").with(snap.instance_id),
            egui::UiBuilder::new()
                .layer_id(layer_id)
                .max_rect(content_rect),
        );
        let sink = PopupSink::new(&snap.plugin_id, snap.instance_id);
        render_popup_tree(&mut child_ui, &snap.tree, &sink, canvas_cache);
        for ev in sink.into_events() {
            state.plugin_popup_events.push((snap.instance_id, ev));
        }

        // 외부 클릭 dismiss.
        if snap.dismiss_on_outside_click
            && primary_pressed
            && let Some(p) = pointer_pos
            && !rect.contains(p)
        {
            state
                .plugin_popup_closes
                .push((snap.instance_id, PopupCloseReason::OutsideClick));
        }
        // Escape dismiss. 여러 popup이 동시에 열려 있을 때 모두 닫는다.
        if escape_pressed {
            state
                .plugin_popup_closes
                .push((snap.instance_id, PopupCloseReason::Escape));
        }
    }

    if any_hovered {
        state.popup_hovered = true;
    }
}

fn anchor_pos(
    anchor: PopupAnchor,
    size: Vec2,
    screen_rect: egui::Rect,
    pointer_pos: Option<egui::Pos2>,
) -> egui::Pos2 {
    let centered = egui::pos2(
        screen_rect.center().x - size.x / 2.0,
        screen_rect.center().y - size.y / 2.0,
    );
    match anchor {
        PopupAnchor::ScreenCenter => centered,
        PopupAnchor::Cursor => pointer_pos.unwrap_or(centered),
        // ActiveSurfaceCenter는 현재 미구현 — 호스트 layout context 통합 필요.
        // 그동안은 ScreenCenter로 fallback.
        PopupAnchor::ActiveSurfaceCenter => centered,
    }
}
