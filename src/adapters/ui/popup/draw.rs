//! `PopupManager::draw` 거대 fn + scope helper 들 분리.

use crate::adapters::ui::LayoutContext;
use crate::theme;
use tasty_type_geometry::length::LogicalPx;

use super::occlusion::{Occluder, PointOwnership, point_ownership};
use super::{PopupDrawResult, PopupId, PopupManager, PopupScope, ResizeEdges};

/// 리사이즈 테두리 밴드 폭(px). popup_rect 가장자리 안쪽 이 폭 안에서 누르면 리사이즈.
const RESIZE_BAND: LogicalPx = LogicalPx(6.0);

/// 포인터가 rect 의 어느 테두리 밴드에 있는지 판정. 어느 엣지에도 안 닿으면 None.
///
/// `band` 가 `LogicalPx` 가 아닌 이유: 본문이 전부 egui `Rect`/`Pos2` 산술이라, 타입을
/// 받으면 호출 한 자리에서 벗기던 것을 본문 네 자리에서 벗기게 된다.
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

/// 텍스트가 `max_width` 를 넘으면 뒤를 `…` 로 잘라 폭 안에 맞춘다(넘지 않으면 원본 그대로).
/// popup 타이틀처럼 가용 폭이 좁을 수 있는 렌더링 경로 공통으로 쓴다 — 개별 popup 이
/// 각자 타이틀 문자열을 축약할 필요 없이 이 함수가 겹침 방지를 전담한다.
fn elide_for_width(ctx: &egui::Context, text: &str, font: egui::FontId, max_width: f32) -> String {
    if max_width <= 0.0 {
        return String::new();
    }
    let width_of = |t: &str| {
        ctx.fonts(|f| {
            f.layout_no_wrap(t.to_owned(), font.clone(), egui::Color32::PLACEHOLDER)
                .rect
                .width()
        })
    };
    if width_of(text) <= max_width {
        return text.to_owned();
    }
    let mut chars: Vec<char> = text.chars().collect();
    while !chars.is_empty() {
        chars.pop();
        let candidate: String = chars.iter().collect::<String>() + "…";
        if width_of(&candidate) <= max_width {
            return candidate;
        }
    }
    "…".to_owned()
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

/// 전체화면 버튼 글리프 — 디자인 아이콘 `fit`([`tasty_icons::FIT`], 네 모서리
/// 브래킷)의 형상을 painter 직선으로 그린다.
///
/// SVG 아이콘(`Icon::image`)을 쓰지 않는 이유: 타이틀바는 `Ui` 가 아니라
/// `ctx.layer_painter` 로만 그려지는 구간이고(콘텐츠 `Area` 는 타이틀바 아래에
/// 따로 열린다), `Image::paint_at` 은 `Ui` 를 요구한다. 바로 옆 close X 도 같은
/// 이유로 painter 직선이라 두 버튼의 렌더 방식이 일치한다.
fn paint_fullscreen_glyph(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    // 디자인 `fit` 은 24 viewBox 안에서 브래킷 사각형이 3~21(=18), 팔 길이 ≈ 5 다.
    // 글리프 자체를 아이콘 크기(버튼의 60%, 옆 close X 와 같은 눈크기)로 잡고 그
    // 안에서 디자인 비례(5/18)를 유지한다 — 버튼 크기가 바뀌어도 형상이 따라간다.
    let g = egui::Rect::from_center_size(rect.center(), egui::Vec2::splat(rect.width() * 0.6));
    let arm = g.width() * (5.0 / 18.0);
    let stroke = egui::Stroke::new(theme::theme().icon_stroke_width.value(), color);
    for (corner, dx, dy) in [
        (g.left_top(), 1.0, 1.0),
        (g.right_top(), -1.0, 1.0),
        (g.left_bottom(), 1.0, -1.0),
        (g.right_bottom(), -1.0, -1.0),
    ] {
        painter.line_segment([corner, corner + egui::vec2(arm * dx, 0.0)], stroke);
        painter.line_segment([corner, corner + egui::vec2(0.0, arm * dy)], stroke);
    }
}

impl PopupManager {
    /// Draw all open popups. The `content_fn` callback is invoked for each popup with its id.
    /// `draw_ctx` provides scope context for visibility and boundary clamping.
    /// Returns draw result including closed popup IDs and hover state for input layer.
    #[allow(clippy::cognitive_complexity)] // complexity-exempt: egui 즉시모드 draw — 열린 popup별 content_fn 콜백 + 경계 clamp, 클로저 중첩이 구조적
    pub fn draw(
        &mut self,
        ctx: &egui::Context,
        content_fn: &mut dyn FnMut(&str, &mut egui::Ui),
        draw_ctx: Option<&LayoutContext>,
        plugin_occluders: &[Occluder],
    ) -> PopupDrawResult {
        let th = theme::theme();
        let screen_rect = ctx.screen_rect();
        let mut closed: Vec<PopupId> = Vec::new();
        let mut bring_front: Option<PopupId> = None;
        let mut fullscreen_requested: Option<crate::adapters::ui::fullscreen::StageId> = None;
        let mut layers: Vec<egui::LayerId> = Vec::new();

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

        // 이번 프레임에 실제로 그려지는 popup 들의 히트테스트 rect — plugin popup 쪽
        // 판정이 같은 프레임에 읽는다(`draw_popups` 가 `draw_plugin_popups` 보다 먼저
        // 돌기 때문에 stale 이 아니다).
        let hit_rects: Vec<Occluder> = open_indices
            .iter()
            .map(|&i| Occluder {
                rect: self.popups[i].popup_rect(),
                z_seq: self.popups[i].z_seq,
            })
            .collect();

        // Determine which popup (topmost) the pointer is over.
        // 우선순위: close/전체화면 버튼 > 리사이즈 엣지 > 드래그 핸들 > 콘텐츠.
        // 전체화면 버튼은 close 와 **같은 층**이다 — 둘 다 매니저가 직접 페인팅한
        // 영역이고 둘 다 타이틀바(드래그 핸들) 위에 겹쳐 있으므로, 같은 우선순위로
        // 핸들보다 먼저 판정해야 버튼을 눌러 끌어도 popup 이 따라오지 않는다.
        let mut hovered_popup: Option<PopupId> = None;
        let mut hovered_handle: Option<PopupId> = None;
        let mut hovered_close: Option<PopupId> = None;
        let mut hovered_fullscreen: Option<(PopupId, crate::adapters::ui::fullscreen::StageId)> =
            None;
        let mut hovered_resize: Option<(PopupId, ResizeEdges)> = None;
        if let Some(pos) = pointer_pos {
            // Check in reverse z-order (topmost first) for correct hit-testing
            for &idx in open_indices.iter().rev() {
                let popup = &self.popups[idx];
                let rect = popup.popup_rect();
                // 규칙 7 — 나보다 위의 plugin popup 이 이 좌표를 덮으면 이 popup 은
                // 포인터를 받지 않는다(hover / click-to-front / close 버튼 전부).
                // 아래(더 낮은 z) host popup 이 대신 hover 를 가져갈 수 있으므로
                // `break` 가 아니라 `continue` 다.
                if matches!(
                    point_ownership(rect, popup.z_seq, plugin_occluders, pos),
                    PointOwnership::OccludedByHigher
                ) {
                    continue;
                }
                if rect.contains(pos) {
                    hovered_popup = Some(popup.id);
                    if !popup.headless && popup.close_btn_rect().contains(pos) {
                        hovered_close = Some(popup.id);
                    } else if let Some((fs_rect, stage)) =
                        popup.fullscreen_btn_rect().zip(popup.fullscreen_stage)
                        && fs_rect.contains(pos)
                    {
                        hovered_fullscreen = Some((popup.id, stage));
                    } else if popup.resizable
                        && let Some(edges) = resize_edges_at(rect, pos, RESIZE_BAND.value())
                    {
                        hovered_resize = Some((popup.id, edges));
                    } else if let Some(handle) = popup.effective_drag_handle_rect(ctx)
                        && handle.contains(pos)
                    {
                        hovered_handle = Some(popup.id);
                    }
                    break; // topmost popup wins
                } else if super::child_overlay_hit(ctx, popup.id, pos) {
                    // draw_fn 이 egui 네이티브 API로 그린 자식 오버레이(드롭다운 등)가
                    // popup_rect 밖으로 삐져나간 경우 — 그 위 클릭은 이 popup 에 대한
                    // "안쪽 클릭"으로 취급한다(close_btn/resize/handle 판정은 popup_rect
                    // 자체에만 유효하므로 여기선 생략).
                    hovered_popup = Some(popup.id);
                    break;
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
            } else if let Some((_, stage)) = hovered_fullscreen {
                // 원본 popup 은 **닫지 않는다** — 무대에 올라가는 것은 이 popup 이
                // 아니라 같은 형상의 별개 콘텐츠이므로, 무대를 나오면 popup 이
                // 그대로 있어야 한다(fullscreen-stage.md §모델 1·2).
                fullscreen_requested = Some(stage);
            } else if let Some(id) = hovered_popup {
                bring_front = Some(id);
                // Focus this popup, unfocus all others
                for popup in &mut self.popups {
                    popup.focused = popup.id == id;
                }
            } else {
                // Clicked outside all *host* popups. 그 좌표를 나보다 위에 있는 plugin
                // egui-mesh popup 이 덮고 있으면 이건 "바깥 클릭" 이 아니라 그 popup 의
                // 클릭이다(규칙 7) — dismiss 도 unfocus 도 하지 않는다.
                //
                // **1 프레임 stale**: host draw 는 `draw_plugin_popups` 보다 먼저 돌아
                // 직전 프레임의 plugin rect 를 본다(반대 방향은 같은 프레임 값이라 정확).
                // 방금 닫힌 plugin popup 이 outside-click 한 번을 더 삼킬 수 있지만,
                // 반대(가려진 popup 이 잘못 닫히는 것)보다 회복이 쉬운 쪽을 택했다.
                for popup in &mut self.popups {
                    let occluded = pointer_pos.is_some_and(|p| {
                        matches!(
                            point_ownership(popup.popup_rect(), popup.z_seq, plugin_occluders, p),
                            PointOwnership::OccludedByHigher
                        )
                    });
                    if occluded {
                        continue;
                    }
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
        // 표시 → sizer 가 크기를 되돌리지 못하게 한다(`popup::frame::draw_popup_layer` 가드).
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

        // Handle request_top — scope rect 상단 가로 중앙 정렬 (margin = spacing-sm).
        for popup in &mut self.popups {
            if popup.request_top && popup.open {
                let anchor_rect = Self::scope_rect(&popup.scope, draw_ctx).unwrap_or(screen_rect);
                popup.pos = egui::pos2(
                    anchor_rect.center().x - popup.size.x / 2.0,
                    anchor_rect.min.y + th.spacing_sm.value(),
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
        } else if hovered_close.is_some() || hovered_fullscreen.is_some() {
            // close / 전체화면 버튼 hover: pointer(손가락) 커서 (디자인 커서 매트릭스).
            ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
        } else if hovered_popup.is_some() {
            // Content area: set default cursor (arrow) to override terminal cursor
            ctx.set_cursor_icon(egui::CursorIcon::Default);
        }

        // --- Render all open popups ---
        let mut scrim_painted = false;
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
            layers.push(layer_id);

            let painter = ctx.layer_painter(layer_id);

            // Scrim: headless 모달 popup(remote_tool/remote_attach/command_palette/
            // port_scanner — popup.rs:303-306 과 동일 id 세트) 뒤 화면 전체를 반투명
            // 검정으로 딤 처리한다(디자인 <Scrim>, plugin_bridge/popup_render.rs:134
            // 패턴 재사용). 여러 개가 동시에 열려 있어도 z-order 최하단 1개(가장 먼저
            // 순회되는 대상)에서만 그려 중첩 딤을 막는다 — 그 popup 자신의 layer 에
            // 배경보다 먼저 그리므로 별도 layer 없이 자연스럽게 그 popup 아래에 깔린다.
            if !scrim_painted
                && matches!(
                    popup_id,
                    "remote_tool"
                        | "remote_attach"
                        | "command_palette"
                        | "port_scanner"
                        | super::transfer::TRANSFER_PROGRESS_POPUP_ID
                        | super::transfer::TRANSFER_ERROR_POPUP_ID
                )
            {
                painter.rect_filled(screen_rect, 0.0, th.scrim().to_egui());
                scrim_painted = true;
            }

            // Popup background. 디자인 semantic 토큰 매핑: 대부분 popup 은
            // surface-raised(=surface0). 단 헤더+리스트형 "패널" popup 은 bg-panel
            // (=base, 한 단계 더 어두움). remote_tool / port_scanner 가 후자.
            let bg_fill: egui::Color32 = match popup_id {
                "remote_tool" | "port_scanner" | "tutorial_topics" | "remote_attach" => {
                    th.bg_panel().into()
                }
                super::transfer::TRANSFER_PROGRESS_POPUP_ID
                | super::transfer::TRANSFER_ERROR_POPUP_ID => th.bg_panel().into(),
                _ => th.surface_raised().into(),
            };
            painter.rect_filled(popup_rect, th.corner_radius.value(), bg_fill);
            painter.rect_stroke(
                popup_rect,
                th.corner_radius.value(),
                egui::Stroke::new(th.border_width.value(), th.border_strong()),
                egui::StrokeKind::Outside,
            );

            if !is_headless {
                let title_rect = popup.title_rect();
                let close_btn_rect = popup.close_btn_rect();
                let fullscreen_btn_rect = popup.fullscreen_btn_rect();
                let buttons_left_x = popup.title_buttons_left_x();

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
                    th.bg_sidebar(),
                );
                painter.line_segment(
                    [
                        egui::pos2(title_rect.min.x, title_rect.max.y),
                        egui::pos2(title_rect.max.x, title_rect.max.y),
                    ],
                    egui::Stroke::new(th.border_width.value(), th.border_strong()),
                );

                // Title text — 우측 버튼군을 침범하지 않는 가용 폭 기준으로 elide.
                // 기준선은 `title_buttons_left_x()` — 버튼이 close 하나면 그 좌변,
                // 전체화면 버튼이 붙으면 그쪽 좌변이라 가용 폭이 버튼 폭 + 간격만큼
                // 줄어든다. (양쪽에 같은 패딩을 둬 버튼과의 간격 + 시각적 대칭 확보.)
                let title_font = egui::FontId::proportional(th.font_size_body.value());
                let title_pad = th.spacing_sm.value();
                let title_avail_rect = egui::Rect::from_min_max(
                    egui::pos2(title_rect.min.x + title_pad, title_rect.min.y),
                    egui::pos2(
                        (buttons_left_x - title_pad).max(title_rect.min.x + title_pad),
                        title_rect.max.y,
                    ),
                );
                let elided_title = elide_for_width(
                    ctx,
                    &popup.title,
                    title_font.clone(),
                    title_avail_rect.width(),
                );
                painter.with_clip_rect(title_avail_rect).text(
                    title_avail_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    &elided_title,
                    title_font,
                    th.text_primary().into(),
                );

                // Fullscreen button — close 왼쪽. `fullscreen_btn_rect()` 가 None 인
                // popup(대부분)에서는 이 블록이 통째로 돌지 않으므로 타이틀바 렌더가
                // 이전과 바이트 단위로 같다.
                if let Some(rect) = fullscreen_btn_rect {
                    let hovered = matches!(hovered_fullscreen, Some((id, _)) if id == popup_id);
                    if hovered {
                        painter.rect_filled(
                            rect,
                            th.corner_radius_sm.value(),
                            th.hover_overlay.to_egui_premultiplied(),
                        );
                    }
                    paint_fullscreen_glyph(
                        &painter,
                        rect,
                        if hovered {
                            th.text_primary().into()
                        } else {
                            th.text_muted().into()
                        },
                    );
                    if hovered {
                        // 아이콘만으로는 뜻이 모호하다. close 와 달리 tooltip 을 다는데,
                        // 매니저가 painter 로 그린 영역이라 `Response::on_hover_text` 가
                        // 없어 egui 의 명시 tooltip API 를 직접 쓴다.
                        egui::show_tooltip_at(
                            ctx,
                            layer_id,
                            egui::Id::new("popup.fullscreen_tooltip").with(popup_id),
                            rect.left_bottom(),
                            |ui| ui.label(crate::i18n::t("popup.fullscreen_button.tooltip")),
                        );
                    }
                }

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
                    th.text_muted()
                };
                let center = close_btn_rect.center();
                painter.line_segment(
                    [
                        center - egui::vec2(x_size, x_size),
                        center + egui::vec2(x_size, x_size),
                    ],
                    egui::Stroke::new(th.icon_stroke_width.value(), x_color),
                );
                painter.line_segment(
                    [
                        center + egui::vec2(-x_size, x_size),
                        center + egui::vec2(x_size, -x_size),
                    ],
                    egui::Stroke::new(th.icon_stroke_width.value(), x_color),
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
            layers,
            fullscreen_requested,
            hit_rects,
        }
    }

    /// Check if a popup's scope is currently visible.
    /// 이번 프레임에 실제로 그려질(open + scope 가시) popup 중 z 가 가장 높은 것.
    /// Esc 소유권 판정용(ADR-0084) — `draw` 안의 `open_indices` 와 같은 필터다.
    pub fn topmost_visible_open(&self, draw_ctx: Option<&LayoutContext>) -> Option<(PopupId, u64)> {
        self.popups
            .iter()
            .filter(|p| p.open && Self::is_scope_visible(&p.scope, draw_ctx))
            .max_by_key(|p| p.z_seq)
            .map(|p| (p.id, p.z_seq))
    }

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

#[cfg(test)]
mod tests {
    use super::*;
    // 픽스처 폰트도 토큰에서 가져온다 — `Theme` 인스턴스가 없는 순수 유닛테스트라
    // zoom 적용 전 원본 상수(`SIZING`)를 쓴다.
    use tasty_type_appearance::theme::SIZING;

    /// `ctx.fonts()` 는 최소 한 프레임(`run`)이 지나야 폰트 정의가 로드된다.
    fn with_ctx<R>(f: impl FnOnce(&egui::Context) -> R) -> R {
        let ctx = egui::Context::default();
        let mut out = None;
        let mut f = Some(f);
        drop(ctx.run(egui::RawInput::default(), |ctx| {
            out = Some((f.take().unwrap())(ctx));
        }));
        out.unwrap()
    }

    #[test]
    fn elide_for_width_keeps_short_text_untouched() {
        with_ctx(|ctx| {
            let font = egui::FontId::proportional(SIZING.font_size_max.value());
            let text = "짧은 제목";
            assert_eq!(elide_for_width(ctx, text, font, 1000.0), text);
        });
    }

    #[test]
    fn elide_for_width_truncates_long_text_with_ellipsis() {
        with_ctx(|ctx| {
            let font = egui::FontId::proportional(SIZING.font_size_max.value());
            let text =
                "파일 핸들러 선택: /Users/ljh/workspace/etc/teams-mcp-very-long-path/Cargo.toml";
            let result = elide_for_width(ctx, text, font, 40.0);
            assert!(result.chars().count() < text.chars().count());
            assert!(result.ends_with('…'));
        });
    }

    #[test]
    fn elide_for_width_zero_or_negative_width_returns_empty() {
        with_ctx(|ctx| {
            let font = egui::FontId::proportional(SIZING.font_size_max.value());
            assert_eq!(elide_for_width(ctx, "anything", font.clone(), 0.0), "");
            assert_eq!(elide_for_width(ctx, "anything", font, -5.0), "");
        });
    }
}
