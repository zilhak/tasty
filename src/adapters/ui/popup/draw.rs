//! `PopupManager::draw` 거대 fn + scope helper 들 분리.

use crate::adapters::ui::LayoutContext;
use crate::theme;

use super::{PopupDrawResult, PopupId, PopupManager, PopupScope, ResizeEdges};

/// 리사이즈 테두리 밴드 폭(px). popup_rect 가장자리 안쪽 이 폭 안에서 누르면 리사이즈.
const RESIZE_BAND: f32 = 6.0;

/// 포인터가 rect 의 어느 테두리 밴드에 있는지 판정. 어느 엣지에도 안 닿으면 None.
fn resize_edges_at(rect: egui::Rect, pos: egui::Pos2, band: f32) -> Option<ResizeEdges> {
    let left = pos.x <= rect.min.x + band;
    let right = pos.x >= rect.max.x - band;
    let top = pos.y <= rect.min.y + band;
    let bottom = pos.y >= rect.max.y - band;
    if left || right || top || bottom {
        Some(ResizeEdges {
            left,
            right,
            top,
            bottom,
        })
    } else {
        None
    }
}

/// 잡은 엣지 조합 → 리사이즈 커서. 모서리는 대각선, 단일 엣지는 수평/수직.
fn resize_cursor(e: ResizeEdges) -> egui::CursorIcon {
    use egui::CursorIcon as C;
    match (e.left, e.right, e.top, e.bottom) {
        (true, _, true, _) => C::ResizeNwSe, // top-left
        (_, true, _, true) => C::ResizeNwSe, // bottom-right
        (_, true, true, _) => C::ResizeNeSw, // top-right
        (true, _, _, true) => C::ResizeNeSw, // bottom-left
        (true, _, _, _) | (_, true, _, _) => C::ResizeHorizontal,
        (_, _, true, _) | (_, _, _, true) => C::ResizeVertical,
        _ => C::Default,
    }
}

impl PopupManager {
    /// Draw all open popups. The `content_fn` callback is invoked for each popup with its id.
    /// `draw_ctx` provides scope context for visibility and boundary clamping.
    /// Returns draw result including closed popup IDs and hover state for input layer.
    pub fn draw(
        &mut self,
        ctx: &egui::Context,
        content_fn: &mut dyn FnMut(&str, &mut egui::Ui),
        draw_ctx: Option<&LayoutContext>,
    ) -> PopupDrawResult {
        let th = theme::theme();
        let screen_rect = ctx.screen_rect();
        let mut closed: Vec<PopupId> = Vec::new();
        let mut bring_front: Option<PopupId> = None;

        // Read pointer state once
        let pointer_pos = ctx.input(|i| i.pointer.interact_pos());
        let primary_pressed = ctx.input(|i| i.pointer.primary_pressed());
        let primary_down = ctx.input(|i| i.pointer.primary_down());
        let primary_released = ctx.input(|i| i.pointer.any_released());

        // Collect open popup indices, filtered by scope visibility
        let open_indices: Vec<usize> = self
            .popups
            .iter()
            .enumerate()
            .filter(|(_, p)| p.open && Self::is_scope_visible(&p.scope, draw_ctx))
            .map(|(i, _)| i)
            .collect();

        // Determine which popup (topmost) the pointer is over.
        // 우선순위: close 버튼 > 리사이즈 엣지 > 드래그 핸들 > 콘텐츠.
        let mut hovered_popup: Option<PopupId> = None;
        let mut hovered_handle: Option<PopupId> = None;
        let mut hovered_close: Option<PopupId> = None;
        let mut hovered_resize: Option<(PopupId, ResizeEdges)> = None;
        if let Some(pos) = pointer_pos {
            // Check in reverse z-order (topmost first) for correct hit-testing
            for &idx in open_indices.iter().rev() {
                let popup = &self.popups[idx];
                let rect = popup.popup_rect();
                if rect.contains(pos) {
                    hovered_popup = Some(popup.id);
                    if !popup.headless && popup.close_btn_rect().contains(pos) {
                        hovered_close = Some(popup.id);
                    } else if popup.resizable
                        && let Some(edges) = resize_edges_at(rect, pos, RESIZE_BAND)
                    {
                        hovered_resize = Some((popup.id, edges));
                    } else if let Some(handle) = popup.drag_handle_rect()
                        && handle.contains(pos)
                    {
                        hovered_handle = Some(popup.id);
                    }
                    break; // topmost popup wins
                }
            }
        }

        // Handle press (pre-content): close > focus/bring-front > outside-click.
        // 이동/리사이즈 START 결정은 콘텐츠 렌더 *뒤* 로 미룬다(아래 post-content
        // 블록) — 위젯 우선 중재(`is_using_pointer`)를 적용하기 위함이다. close 는
        // 매니저가 직접 페인팅한 영역이라 egui 위젯이 아니므로 여기서 처리한다.
        // focus/bring_front 는 START 여부와 무관(같은 팝업)하므로 여기서 끝낸다.
        if primary_pressed {
            if let Some(id) = hovered_close {
                closed.push(id);
            } else if let Some(id) = hovered_popup {
                bring_front = Some(id);
                // Focus this popup, unfocus all others
                for popup in &mut self.popups {
                    popup.focused = popup.id == id;
                }
            } else {
                // Clicked outside all popups
                for popup in &mut self.popups {
                    if popup.open && popup.close_on_outside_click {
                        closed.push(popup.id);
                    }
                    // sticky_focus popups keep keyboard focus when clicking outside.
                    if !popup.sticky_focus {
                        popup.focused = false;
                    }
                }
            }
        }

        // Handle drag move / release
        for popup in &mut self.popups {
            if !popup.dragging {
                continue;
            }
            if primary_released {
                popup.dragging = false;
            } else if primary_down && let Some(pos) = pointer_pos {
                let bounds = Self::scope_rect(&popup.scope, draw_ctx).unwrap_or(screen_rect);
                let new_pos = pos - popup.drag_offset;
                popup.pos = egui::pos2(
                    new_pos.x.clamp(
                        bounds.min.x,
                        (bounds.max.x - popup.size.x).max(bounds.min.x),
                    ),
                    new_pos.y.clamp(
                        bounds.min.y,
                        (bounds.max.y - popup.size.y).max(bounds.min.y),
                    ),
                );
            }
        }

        // Handle resize move / release. 잡은 엣지만 이동(반대편 고정), min_size 클램프 후
        // scope 경계로 클램프. 사용자 리사이즈가 발생하면 size_user_overridden=true 로
        // 표시 → sizer 가 크기를 되돌리지 못하게 한다(notification.rs 가드).
        for popup in &mut self.popups {
            let Some(edges) = popup.resizing else {
                continue;
            };
            if primary_released {
                popup.resizing = None;
                continue;
            }
            if primary_down && let Some(pos) = pointer_pos {
                let start = popup.resize_start_rect;
                let mut min = start.min;
                let mut max = start.max;
                if edges.left {
                    min.x = pos.x;
                }
                if edges.right {
                    max.x = pos.x;
                }
                if edges.top {
                    min.y = pos.y;
                }
                if edges.bottom {
                    max.y = pos.y;
                }
                // min_size 클램프 — 잡은 엣지를 반대편 고정 엣지 기준으로 제한.
                let mw = popup.min_size.x;
                let mh = popup.min_size.y;
                if edges.left {
                    min.x = min.x.min(max.x - mw);
                }
                if edges.right {
                    max.x = max.x.max(min.x + mw);
                }
                if edges.top {
                    min.y = min.y.min(max.y - mh);
                }
                if edges.bottom {
                    max.y = max.y.max(min.y + mh);
                }
                let bounds = Self::scope_rect(&popup.scope, draw_ctx).unwrap_or(screen_rect);
                let new_rect = egui::Rect::from_min_max(min, max).intersect(bounds);
                popup.pos = new_rect.min;
                popup.size = new_rect.size();
                popup.size_user_overridden = true;
            }
        }

        // Handle request_center (use scope rect if available, else screen rect)
        for popup in &mut self.popups {
            if popup.request_center && popup.open {
                let center_rect = Self::scope_rect(&popup.scope, draw_ctx).unwrap_or(screen_rect);
                popup.pos = egui::pos2(
                    center_rect.center().x - popup.size.x / 2.0,
                    center_rect.center().y - popup.size.y / 2.0,
                );
                popup.request_center = false;
            }
        }

        // Handle request_top — scope rect 상단 가로 중앙 정렬 (margin 8px).
        for popup in &mut self.popups {
            if popup.request_top && popup.open {
                let anchor_rect = Self::scope_rect(&popup.scope, draw_ctx).unwrap_or(screen_rect);
                popup.pos = egui::pos2(
                    anchor_rect.center().x - popup.size.x / 2.0,
                    anchor_rect.min.y + 8.0,
                );
                popup.request_top = false;
            }
        }

        // Set cursor. 진행 중인 리사이즈/드래그가 우선(포인터가 밴드 밖으로 나가도 유지),
        // 그 다음 hover 상태.
        let active_resize = self
            .popups
            .iter()
            .find_map(|p| if p.dragging { None } else { p.resizing });
        let active_drag = self.popups.iter().any(|p| p.dragging);
        if let Some(edges) = active_resize {
            ctx.set_cursor_icon(resize_cursor(edges));
        } else if active_drag {
            ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
        } else if let Some((_, edges)) = hovered_resize {
            ctx.set_cursor_icon(resize_cursor(edges));
        } else if hovered_handle.is_some() {
            ctx.set_cursor_icon(egui::CursorIcon::Grab);
        } else if hovered_popup.is_some() && hovered_close.is_none() {
            // Content area: set default cursor (arrow) to override terminal cursor
            ctx.set_cursor_icon(egui::CursorIcon::Default);
        }

        // --- Render all open popups ---
        for (z_idx, &popup_idx) in open_indices.iter().enumerate() {
            let popup = &mut self.popups[popup_idx];
            if closed.contains(&popup.id) {
                continue;
            }

            let clamp_rect = Self::scope_rect(&popup.scope, draw_ctx).unwrap_or(screen_rect);
            popup.clamp_to_screen(clamp_rect);

            let popup_id = popup.id;
            let is_headless = popup.headless;
            let popup_rect = popup.popup_rect();
            let content_rect = popup.content_rect();

            let layer_id = egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::new("popup").with(popup_id).with(z_idx),
            );

            let painter = ctx.layer_painter(layer_id);

            // Popup background. 디자인 semantic 토큰 매핑: 대부분 popup 은
            // surface-raised(=surface0). 단 헤더+리스트형 "패널" popup 은 bg-panel
            // (=base, 한 단계 더 어두움). remote_tool / port_scanner 가 후자.
            let bg_fill: egui::Color32 = match popup_id {
                "remote_tool" | "port_scanner" => th.base.into(),
                _ => th.surface0.into(),
            };
            painter.rect_filled(popup_rect, th.corner_radius.value(), bg_fill);
            painter.rect_stroke(
                popup_rect,
                th.corner_radius.value(),
                egui::Stroke::new(th.border_width.value(), th.surface1),
                egui::StrokeKind::Outside,
            );

            if !is_headless {
                let title_rect = popup.title_rect();
                let close_btn_rect = popup.close_btn_rect();

                // Title bar
                let cr = th.corner_radius.value() as u8;
                painter.rect_filled(
                    title_rect,
                    egui::CornerRadius {
                        nw: cr,
                        ne: cr,
                        sw: 0,
                        se: 0,
                    },
                    th.mantle,
                );
                painter.line_segment(
                    [
                        egui::pos2(title_rect.min.x, title_rect.max.y),
                        egui::pos2(title_rect.max.x, title_rect.max.y),
                    ],
                    egui::Stroke::new(th.border_width.value(), th.surface1),
                );

                // Title text (centered)
                painter.text(
                    title_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    &popup.title,
                    egui::FontId::proportional(th.font_size_body.value()),
                    th.text.into(),
                );

                // Close button
                let is_close_hovered = hovered_close == Some(popup_id);
                if is_close_hovered {
                    painter.rect_filled(
                        close_btn_rect,
                        2.0,
                        th.hover_overlay.to_egui_premultiplied(),
                    );
                }
                let x_size = 5.0;
                let x_color = if is_close_hovered {
                    th.accent_danger()
                } else {
                    th.subtext0
                };
                let center = close_btn_rect.center();
                painter.line_segment(
                    [
                        center - egui::vec2(x_size, x_size),
                        center + egui::vec2(x_size, x_size),
                    ],
                    egui::Stroke::new(1.5, x_color),
                );
                painter.line_segment(
                    [
                        center + egui::vec2(-x_size, x_size),
                        center + egui::vec2(x_size, -x_size),
                    ],
                    egui::Stroke::new(1.5, x_color),
                );
            }

            // Content — egui::Area 로 등록해야 egui 의 layer_id_at(스크롤/호버 라우팅)이
            // 팝업을 인식한다. 이전엔 bare `Ui::new(layer_id)` 라 Area 미등록 → layer_id_at
            // 이 팝업 레이어를 못 찾음 → ScrollArea 의 ui_contains_pointer()=false →
            // 휠/드래그 스크롤 입력이 무시됐다. Area id 를 bg painter 와 동일한 layer_id 의
            // Id 로 맞춰 같은 레이어를 공유(bg→content z-order 자동 정합).
            // movable(false): tasty 가 수동 드래그. sense(hover): layer_id_at 등록만,
            // 클릭/드래그는 내부 위젯이 처리하게 둠.
            {
                let area_id = egui::Id::new("popup").with(popup_id).with(z_idx);
                egui::Area::new(area_id)
                    .order(egui::Order::Foreground)
                    .fixed_pos(content_rect.min)
                    .movable(false)
                    .interactable(true)
                    .sense(egui::Sense::hover())
                    .constrain(false)
                    .show(ctx, |ui| {
                        // Area 의 hit-rect 를 content_rect 전체로 강제한다. set_min_size 가
                        // 없으면 Area 가 콘텐츠(헤더+필터 등)에 맞춰 auto-shrink → footer
                        // (allocate_new_ui 로 별도 배치)와 빈 공간이 빠져 hit-rect 가 줄고
                        // layer_id_at 이 팝업 하단을 인식 못 한다.
                        ui.set_min_size(content_rect.size());
                        ui.set_max_size(content_rect.size());
                        // 이전 `Ui::new(max_rect(content_rect))` 는 clip_rect=content_rect
                        // 라 콘텐츠 넘침(State 컬럼의 긴 라벨, 선택 하이라이트, 스크롤바)이
                        // 팝업 경계에서 잘렸다. Area 는 기본 clip 이 더 넓어 넘침이 팝업
                        // 밖으로 샌다 → content_rect 로 clip 복원.
                        ui.set_clip_rect(content_rect);
                        content_fn(popup_id, ui);
                    });
            }
        }

        // Handle drag/resize START (post-content): 위젯 우선 중재.
        // `ctx.is_using_pointer()` 는 이번 프레임에 어떤 egui 위젯이 이 프레스를
        // 가져갔는지(potential_click/drag_id) 반영하며, 콘텐츠 렌더 *후* 에야
        // 확정된다. 어떤 위젯도 프레스를 가져가지 않았을 때만 이동/리사이즈를
        // 시작한다 → 헤더 드래그 띠가 검색 입력 등 위젯과 겹쳐도 위젯이 항상
        // 우선(명세 입력 우선순위: 위젯 > 리사이즈 > 이동). 우리 수동 드래그는
        // egui 위젯이 아니라 이 신호를 self-trigger 하지 않는다. focus/bring_front
        // 는 위 pre-content 블록에서 이미 처리됨. (close 는 매니저 페인팅이라
        // is_using_pointer 에 안 잡히므로 pre-content 에서 따로 처리해 우선됨.)
        if primary_pressed && !ctx.is_using_pointer() {
            if let Some((id, edges)) = hovered_resize {
                if let Some(popup) = self.popups.iter_mut().find(|p| p.id == id) {
                    popup.resizing = Some(edges);
                    popup.resize_start_rect = popup.popup_rect();
                }
            } else if let Some(id) = hovered_handle
                && let Some(popup) = self.popups.iter_mut().find(|p| p.id == id)
            {
                popup.dragging = true;
                if let Some(pos) = pointer_pos {
                    popup.drag_offset = pos - popup.pos;
                }
            }
        }

        // Apply close
        for id in &closed {
            self.close(id);
        }

        // Bring clicked popup to front
        if let Some(id) = bring_front {
            self.bring_to_front(id);
        }

        PopupDrawResult {
            closed,
            hovered: hovered_popup.is_some(),
        }
    }

    /// Check if a popup's scope is currently visible.
    fn is_scope_visible(scope: &PopupScope, ctx: Option<&LayoutContext>) -> bool {
        let Some(ctx) = ctx else { return true };
        match scope {
            PopupScope::Window => true,
            PopupScope::Workspace(ws_idx) => *ws_idx == ctx.active_workspace,
            PopupScope::Pane(pane_id) => ctx.pane_rects.iter().any(|(id, _)| *id == *pane_id),
            PopupScope::Tab(pane_id, tab_idx) => ctx
                .active_tabs
                .iter()
                .any(|(pid, tidx)| *pid == *pane_id && *tidx == *tab_idx),
            PopupScope::Surface(surface_id) => {
                ctx.surface_rects.iter().any(|(id, _)| *id == *surface_id)
            }
        }
    }

    /// Get the bounding rect for a popup's scope.
    fn scope_rect(scope: &PopupScope, ctx: Option<&LayoutContext>) -> Option<egui::Rect> {
        let ctx = ctx?;
        match scope {
            PopupScope::Window => None,       // use screen_rect (caller default)
            PopupScope::Workspace(_) => None, // workspace fills window
            PopupScope::Pane(pane_id) => ctx
                .pane_rects
                .iter()
                .find(|(id, _)| *id == *pane_id)
                .map(|(_, r)| *r),
            PopupScope::Tab(pane_id, _) => ctx
                .pane_rects
                .iter()
                .find(|(id, _)| *id == *pane_id)
                .map(|(_, r)| *r),
            PopupScope::Surface(surface_id) => ctx
                .surface_rects
                .iter()
                .find(|(id, _)| *id == *surface_id)
                .map(|(_, r)| *r),
        }
    }
}
