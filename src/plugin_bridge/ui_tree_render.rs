//! `UiNode` → egui 위젯 렌더링.
//!
//! 사용자 입력은 [`UiSink::push_event`]를 통해 호스트가 보관해 두었다가 다음 pump tick에
//! plugin에 송신한다. surface([`RemoteSurface`])와 popup 인스턴스 모두 동일 렌더러를
//! 공유하기 위해 sink 추상화를 사용한다.

use std::cell::RefCell;

use egui::Ui;

use crate::gpu::canvas_texture::CanvasTextureCache;
use crate::plugin::ui_tree::{
    BadgeTone, ButtonStyle, CanvasPointerButton, CanvasPointerPhase, LabelStyle, SharedBufferId,
    SplitDir, TagTone, TreeNode, UiEvent, UiNode,
};
use crate::plugin_bridge::remote_surface::RemoteSurface;

/// 렌더러가 plugin tree를 그리는 동안 사용하는 추상 sink.
///
/// - `push_event`: 사용자 입력을 plugin에 보낼 큐에 적재.
/// - `plugin_id`: canvas 텍스처 캐시 lookup 키.
/// - `salt`: egui memory id 충돌 방지용 disambiguator. surface는 `SurfaceId`, popup은
///   `instance_id`를 사용.
pub trait UiSink {
    fn push_event(&self, event: UiEvent);
    fn plugin_id(&self) -> &str;
    fn salt(&self) -> u64;
}

impl UiSink for RemoteSurface {
    fn push_event(&self, event: UiEvent) {
        RemoteSurface::push_event(self, event);
    }
    fn plugin_id(&self) -> &str {
        &self.plugin_id
    }
    fn salt(&self) -> u64 {
        self.id as u64
    }
}

/// Popup 한 인스턴스의 입력 sink. 매 프레임 새로 만들고, 렌더 종료 후 `into_events()`로
/// 모아진 이벤트를 꺼내 `PluginManager::send_popup_event`로 dispatch.
pub struct PopupSink<'a> {
    plugin_id: &'a str,
    salt: u64,
    events: RefCell<Vec<UiEvent>>,
}

impl<'a> PopupSink<'a> {
    pub fn new(plugin_id: &'a str, instance_id: u64) -> Self {
        Self {
            plugin_id,
            salt: instance_id,
            events: RefCell::new(Vec::new()),
        }
    }

    pub fn into_events(self) -> Vec<UiEvent> {
        self.events.into_inner()
    }
}

impl<'a> UiSink for PopupSink<'a> {
    fn push_event(&self, event: UiEvent) {
        self.events.borrow_mut().push(event);
    }
    fn plugin_id(&self) -> &str {
        self.plugin_id
    }
    fn salt(&self) -> u64 {
        self.salt
    }
}

pub fn render_remote_surface(
    ui: &mut Ui,
    surface: &RemoteSurface,
    canvas_cache: &CanvasTextureCache,
) {
    let tree_opt = surface.tree.lock().ok().and_then(|t| t.clone());
    match tree_opt {
        Some(node) => render_node(ui, &node, surface, canvas_cache),
        None => {
            ui.vertical_centered(|ui| {
                ui.add_space(20.0);
                ui.label(
                    egui::RichText::new(format!("Loading plugin surface: {}", surface.kind_static))
                        .italics(),
                );
                ui.label(
                    egui::RichText::new(&surface.plugin_id)
                        .small()
                        .color(crate::theme::theme().subtext0),
                );
            });
        }
    }
}

/// Plugin popup의 UI tree를 렌더한다. surface와 달리 popup 인스턴스는 호스트 측
/// `PluginPopupInstance.tree`에 직접 보관된 [`UiNode`]를 그대로 받는다.
pub fn render_popup_tree(
    ui: &mut Ui,
    tree: &UiNode,
    sink: &PopupSink<'_>,
    canvas_cache: &CanvasTextureCache,
) {
    render_node(ui, tree, sink, canvas_cache);
}

fn render_node(ui: &mut Ui, node: &UiNode, sink: &dyn UiSink, canvas_cache: &CanvasTextureCache) {
    match node {
        UiNode::Vbox { spacing, children } => {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = *spacing as f32;
                for (i, c) in children.iter().enumerate() {
                    // 자식별로 push_id로 id_salt를 분리 → 같은 종류의 stateful
                    // 위젯(ScrollArea, CollapsingHeader 등)이 형제 위치에 있어도
                    // egui ID가 충돌하지 않는다.
                    ui.push_id(i, |ui| render_node(ui, c, sink, canvas_cache));
                }
            });
        }
        UiNode::Hbox { spacing, children } => {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = *spacing as f32;
                for (i, c) in children.iter().enumerate() {
                    ui.push_id(i, |ui| render_node(ui, c, sink, canvas_cache));
                }
            });
        }
        UiNode::Center { child } => {
            // Splitter(L534/595) 와 동일 관용구: 가용 rect 를 통째로 잡고 양축
            // 중앙(centered_and_justified)으로 자식 1개를 배치한다.
            let rect = ui.available_rect_before_wrap();
            ui.scope_builder(
                egui::UiBuilder::new()
                    .max_rect(rect)
                    .layout(egui::Layout::centered_and_justified(
                        egui::Direction::TopDown,
                    )),
                |ui| render_node(ui, child, sink, canvas_cache),
            );
            ui.advance_cursor_after_rect(rect);
        }
        UiNode::Scroll {
            vertical,
            horizontal,
            child,
        } => {
            // auto_shrink=false: 부모(splitter 등)가 할당한 max_rect 전체를 채우게 한다.
            // 기본값(true)은 콘텐츠 크기로 축소되어 splitter 우측 영역이 시각적으로 사라진다.
            egui::ScrollArea::new([*horizontal, *vertical])
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    render_node(ui, child, sink, canvas_cache);
                });
        }
        UiNode::Splitter {
            direction,
            ratio,
            first,
            second,
            id,
        } => {
            render_splitter(
                ui,
                *direction,
                *ratio,
                id.as_deref(),
                first,
                second,
                sink,
                canvas_cache,
            );
        }
        UiNode::Label { text, style, color } => {
            let mut rt = egui::RichText::new(text);
            {
                // 디자인 토큰: heading = font_size_heading(13) semibold + text_primary,
                // caption = font_size_caption(11), dim = text_muted. (transcription-spec §2-C)
                let th = crate::theme::theme();
                rt = match style {
                    LabelStyle::Body => rt,
                    LabelStyle::Caption => rt.size(th.font_size_caption.value()),
                    LabelStyle::Heading => rt
                        .size(th.font_size_heading.value())
                        .color(th.text_primary().to_egui())
                        .strong(),
                    LabelStyle::Dim => rt.color(th.text_muted().to_egui()),
                    LabelStyle::Mono => rt.monospace(),
                };
            }
            if let Some(c) = color.as_deref()
                && let Some(parsed) = parse_color_token(c)
            {
                rt = rt.color(parsed);
            }
            ui.label(rt);
        }
        UiNode::Icon { name } => {
            ui.label(name);
        }
        UiNode::Button {
            id,
            label,
            enabled,
            style,
            block,
            tooltip_i18n_key,
        } => {
            // 디자인 Button 위젯으로 라우팅 — Primary→accent 채움, Secondary(기본)→
            // surface_raised + border_default 외곽선. (transcription-spec §2-B)
            let variant = match style {
                ButtonStyle::Primary => tasty_ui_widgets::ButtonVariant::Primary,
                ButtonStyle::Secondary => tasty_ui_widgets::ButtonVariant::Secondary,
            };
            let th = crate::theme::theme();
            let resp = tasty_ui_widgets::Button::new(label)
                .variant(variant)
                .enabled(*enabled)
                .block(*block)
                .show(ui, &th);
            let resp = match tooltip_i18n_key {
                Some(key) => resp.on_hover_text(crate::i18n::t(key)),
                None => resp,
            };
            if resp.clicked() {
                sink.push_event(UiEvent::Click {
                    node_id: id.clone(),
                });
            }
        }
        UiNode::Tree {
            id,
            nodes,
            selection_mode: _,
        } => {
            for n in nodes {
                render_tree_node(ui, id, n, "", sink);
            }
        }
        UiNode::Addressbar {
            id,
            text,
            placeholder_i18n_key,
        } => {
            // 사용자 편집 buffer 와 직전 protocol text 를 egui memory 에 보관해
            // frame 경계 너머로 보존한다. plugin 측 text 가 실제로 바뀐 경우에만
            // 사용자 buffer 를 무효화 — splitter ratio (render_splitter) 와 동일 패턴.
            let (user_buf_id, last_protocol_id) = addressbar_memory_ids(sink, id);
            let stored = ui.ctx().memory(|m| {
                (
                    m.data.get_temp::<String>(user_buf_id),
                    m.data.get_temp::<String>(last_protocol_id),
                )
            });
            let mut buf = match stored {
                (Some(user), Some(last)) if last == *text => user,
                _ => text.clone(),
            };
            let placeholder = placeholder_i18n_key
                .as_ref()
                .map(|k| crate::i18n::t(k))
                .unwrap_or_default();
            let resp = ui.add(
                egui::TextEdit::singleline(&mut buf)
                    .hint_text(tasty_egui_theme::hint_text(
                        &crate::theme::theme(),
                        placeholder,
                    ))
                    .desired_width(f32::INFINITY),
            );
            if resp.changed() {
                sink.push_event(UiEvent::AddressbarChange {
                    node_id: id.clone(),
                    text: buf.clone(),
                });
            }
            if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                sink.push_event(UiEvent::AddressbarSubmit {
                    node_id: id.clone(),
                    text: buf.clone(),
                });
            }
            ui.ctx().memory_mut(|m| {
                m.data.insert_temp(user_buf_id, buf);
                m.data.insert_temp(last_protocol_id, text.clone());
            });
        }
        UiNode::TextPreview {
            content,
            language: _,
        } => {
            // syntax highlighting은 향후 — 현 단계는 plain monospace.
            // surface 콘텐츠 규칙: mono text_primary, 좌·상 spacing_sm 인셋. (spec §2-E / C-G5)
            let (inset, color) = {
                let th = crate::theme::theme();
                (th.spacing_sm.value() as i8, th.text_primary().to_egui())
            };
            egui::Frame::NONE
                .inner_margin(egui::Margin {
                    left: inset,
                    top: inset,
                    right: 0,
                    bottom: 0,
                })
                .show(ui, |ui| {
                    ui.add(
                        egui::Label::new(egui::RichText::new(content).monospace().color(color))
                            .wrap(),
                    );
                });
        }
        UiNode::Spacer { size } => {
            ui.add_space(*size as f32);
        }
        UiNode::Tag { text, tone, dot } => {
            let th = crate::theme::theme();
            tasty_ui_widgets::tag(ui, &th, text, tag_variant(*tone), *dot);
        }
        UiNode::Badge { text, tone, dot } => {
            let th = crate::theme::theme();
            if *dot {
                tasty_ui_widgets::badge_dot(ui, &th, badge_variant(*tone));
            } else {
                tasty_ui_widgets::badge(ui, &th, text, badge_variant(*tone));
            }
        }
        UiNode::Canvas {
            buffer_id,
            width,
            height,
            format: _,
            filter: _,
            commit_seq: _,
            id,
        } => {
            render_canvas(
                ui,
                sink,
                canvas_cache,
                *buffer_id,
                *width,
                *height,
                id.as_deref(),
            );
        }
        UiNode::SelectableRow {
            id,
            selected,
            children,
        } => {
            // 전체 폭을 차지하는 클릭 가능한 행. TreeRow 토큰 레시피 이식:
            // selected → surface_active(불투명) fill, hover → overlay_hover 오버레이,
            // radius_sm, 마진 spacing_xs. (transcription-spec §2-A(c))
            // SelectableRow 는 임의 children 컨테이너라 tree_row() 드롭인 불가 → 레시피만 이식.
            let (radius, margin, gap, selected_fill, hover_fill) = {
                let th = crate::theme::theme();
                let xs = th.spacing_xs.value();
                (
                    th.corner_radius_sm.value(),
                    egui::Margin::symmetric(xs as i8, (xs * 0.5) as i8),
                    xs,
                    th.surface_active().to_egui(),
                    th.overlay_hover().to_egui_premultiplied(),
                )
            };
            // 배경 shape 를 먼저 예약해 콘텐츠 뒤에(behind) 그린다 — hover 응답을 얻은
            // 뒤에야 fill 색을 알 수 있으므로 reserve-then-set 패턴 사용.
            let bg_idx = ui.painter().add(egui::Shape::Noop);
            let resp = egui::Frame::NONE
                .inner_margin(margin)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = gap;
                        for (i, c) in children.iter().enumerate() {
                            ui.push_id(i, |ui| render_node(ui, c, sink, canvas_cache));
                        }
                    });
                })
                .response
                .interact(egui::Sense::click());
            let fill = if *selected {
                selected_fill
            } else if resp.hovered() {
                hover_fill
            } else {
                egui::Color32::TRANSPARENT
            };
            if fill != egui::Color32::TRANSPARENT {
                ui.painter()
                    .set(bg_idx, egui::Shape::rect_filled(resp.rect, radius, fill));
            }
            if resp.clicked() {
                sink.push_event(UiEvent::Click {
                    node_id: id.clone(),
                });
            }
        }
    }
}

/// Plugin Canvas node 렌더링 + 마우스 이벤트 dispatch.
///
/// - cache에 등록된 [`egui::TextureId`]가 있으면 [`egui::Image`]로 그린다 (03e가 cache를 채움).
///   없으면 자리 표시자 회색 사각형으로 placehold.
/// - `id`가 있으면 click/drag/hover를 [`UiEvent::CanvasPointer`]로 plugin에 송신.
/// - 좌표는 canvas-local 픽셀 좌표 (0..width, 0..height). egui는 logical px이므로 canvas
///   rect 내 normalize 후 `width`/`height`를 곱해 픽셀 좌표로 변환한다.
/// - Move/Drag는 frame 당 자연스럽게 1회 emit (egui Response의 invariant). Leave는
///   직전 frame의 hovered 상태를 egui memory에 저장해 false 전이 시점에 송신.
fn render_canvas(
    ui: &mut Ui,
    sink: &dyn UiSink,
    canvas_cache: &CanvasTextureCache,
    buffer_id: SharedBufferId,
    width: u32,
    height: u32,
    node_id: Option<&str>,
) {
    let size = egui::vec2(width as f32, height as f32);
    let sense = if node_id.is_some() {
        egui::Sense::click_and_drag()
    } else {
        egui::Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(size, sense);

    // 1. 그리기: cache 등록 텍스처가 있으면 Image, 없으면 placeholder.
    match canvas_cache.get(sink.plugin_id(), buffer_id) {
        Some(tex_id) => {
            ui.painter().image(
                tex_id,
                rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
        None => {
            // canvas 미할당 시 placeholder fill — theme.surface0 사용.
            ui.painter()
                .rect_filled(rect, 0.0, crate::theme::theme().surface0);
        }
    }

    // 2. 이벤트 dispatch (id가 있을 때만).
    let Some(node_id) = node_id else {
        return;
    };

    // canvas-local 픽셀 좌표 변환: ui 좌표(rect) → (0..width, 0..height).
    let to_canvas_local = |p: egui::Pos2| -> (f32, f32) {
        let nx = ((p.x - rect.min.x) / rect.width().max(1.0)).clamp(0.0, 1.0);
        let ny = ((p.y - rect.min.y) / rect.height().max(1.0)).clamp(0.0, 1.0);
        (nx * width as f32, ny * height as f32)
    };

    // Click → Down + Up (primary). egui Response는 클릭 완료 시 한 번만 true.
    if resp.clicked() {
        if let Some(p) = resp.interact_pointer_pos() {
            let (x, y) = to_canvas_local(p);
            sink.push_event(UiEvent::CanvasPointer {
                node_id: node_id.to_string(),
                x,
                y,
                phase: CanvasPointerPhase::Down,
                button: Some(CanvasPointerButton::Primary),
            });
            sink.push_event(UiEvent::CanvasPointer {
                node_id: node_id.to_string(),
                x,
                y,
                phase: CanvasPointerPhase::Up,
                button: Some(CanvasPointerButton::Primary),
            });
        }
    } else if resp.dragged() {
        if let Some(p) = resp.interact_pointer_pos() {
            let (x, y) = to_canvas_local(p);
            sink.push_event(UiEvent::CanvasPointer {
                node_id: node_id.to_string(),
                x,
                y,
                phase: CanvasPointerPhase::Drag,
                button: Some(CanvasPointerButton::Primary),
            });
        }
    } else if resp.hovered() {
        // 단순 hover: 포인터 위치만 Move로 전달. egui Response는 frame당 한번만 hovered=true,
        // 그리고 Move 이벤트는 캐주얼한 위젯에서 흔하지 않으므로 throttle 자체는 충분.
        if let Some(p) = ui.ctx().input(|i| i.pointer.hover_pos()) {
            let (x, y) = to_canvas_local(p);
            sink.push_event(UiEvent::CanvasPointer {
                node_id: node_id.to_string(),
                x,
                y,
                phase: CanvasPointerPhase::Move,
                button: None,
            });
        }
    }

    // Leave 감지: 직전 frame에 hovered였는데 이번 frame은 아니면 송신.
    let mem_id = egui::Id::new(("canvas_hovered", sink.salt(), node_id));
    let was_hovered: bool = ui
        .ctx()
        .memory(|m| m.data.get_temp(mem_id).unwrap_or(false));
    let is_hovered = resp.hovered() || resp.dragged() || resp.clicked();
    if was_hovered && !is_hovered {
        // 마지막으로 알려진 hover 위치가 없을 수 있으므로 (-1, -1) 대신 영역 외 표시.
        // 보통 plugin은 phase만 보고 추적 상태를 초기화한다.
        sink.push_event(UiEvent::CanvasPointer {
            node_id: node_id.to_string(),
            x: -1.0,
            y: -1.0,
            phase: CanvasPointerPhase::Leave,
            button: None,
        });
    }
    ui.ctx()
        .memory_mut(|m| m.data.insert_temp(mem_id, is_hovered));
}

/// Splitter 렌더링. `id`가 `Some`이면 divider를 드래그해 비율을 조절할 수 있으며,
/// 변경된 ratio는 매 프레임 plugin에 `SplitterDrag` 이벤트로 전달된다.
///
/// 부드러운 시각 피드백을 위해 사용자가 드래그한 ratio를 egui 메모리에 저장해
/// 다음 프레임 plugin 응답이 오기 전에도 즉시 반영한다. plugin이 동일한 ratio로
/// 다시 그려 보내면 메모리 값이 보존되고, 다른 ratio로 보내면(plugin 측 clamp 등)
/// 그 값으로 동기화된다.
#[allow(clippy::too_many_arguments)]
fn render_splitter(
    ui: &mut Ui,
    direction: SplitDir,
    protocol_ratio: f32,
    id: Option<&str>,
    first: &UiNode,
    second: &UiNode,
    sink: &dyn UiSink,
    canvas_cache: &CanvasTextureCache,
) {
    const HANDLE_THICKNESS: f32 = 6.0;
    const MIN_PANE_PX: f32 = 40.0;

    let avail = ui.available_rect_before_wrap();
    let effective_ratio = if let Some(id) = id {
        let (mem_id, last_protocol_id) = splitter_memory_ids(sink, id);
        let ctx = ui.ctx();
        let stored = ctx.memory(|m| {
            (
                m.data.get_temp::<f32>(mem_id),
                m.data.get_temp::<f32>(last_protocol_id),
            )
        });
        // protocol ratio가 바뀌면 사용자 메모리를 새 값으로 동기화 — plugin이
        // clamp / 외부 변경한 경우를 반영.
        let user_ratio = match stored {
            (Some(user), Some(last)) if (last - protocol_ratio).abs() < f32::EPSILON => user,
            _ => protocol_ratio,
        };
        ctx.memory_mut(|m| {
            m.data.insert_temp(mem_id, user_ratio);
            m.data.insert_temp(last_protocol_id, protocol_ratio);
        });
        user_ratio
    } else {
        protocol_ratio
    };
    let r = effective_ratio.clamp(0.05, 0.95);

    let (first_rect, second_rect, handle_rect, axis_size, axis_min) = match direction {
        SplitDir::Horizontal => {
            let split_x = avail.min.x + avail.width() * r;
            let first_rect = egui::Rect::from_min_max(avail.min, egui::pos2(split_x, avail.max.y));
            let second_rect = egui::Rect::from_min_max(egui::pos2(split_x, avail.min.y), avail.max);
            let handle_rect = egui::Rect::from_min_max(
                egui::pos2(split_x - HANDLE_THICKNESS * 0.5, avail.min.y),
                egui::pos2(split_x + HANDLE_THICKNESS * 0.5, avail.max.y),
            );
            (
                first_rect,
                second_rect,
                handle_rect,
                avail.width(),
                avail.min.x,
            )
        }
        SplitDir::Vertical => {
            let split_y = avail.min.y + avail.height() * r;
            let first_rect = egui::Rect::from_min_max(avail.min, egui::pos2(avail.max.x, split_y));
            let second_rect = egui::Rect::from_min_max(egui::pos2(avail.min.x, split_y), avail.max);
            let handle_rect = egui::Rect::from_min_max(
                egui::pos2(avail.min.x, split_y - HANDLE_THICKNESS * 0.5),
                egui::pos2(avail.max.x, split_y + HANDLE_THICKNESS * 0.5),
            );
            (
                first_rect,
                second_rect,
                handle_rect,
                avail.height(),
                avail.min.y,
            )
        }
    };

    ui.scope_builder(egui::UiBuilder::new().max_rect(first_rect), |ui| {
        ui.push_id("split_first", |ui| {
            render_node(ui, first, sink, canvas_cache)
        });
    });
    ui.scope_builder(egui::UiBuilder::new().max_rect(second_rect), |ui| {
        ui.push_id("split_second", |ui| {
            render_node(ui, second, sink, canvas_cache)
        });
    });

    if let Some(id_str) = id {
        let handle_id = ui.make_persistent_id(("splitter_handle", sink.salt(), id_str));
        let resp = ui.interact(handle_rect, handle_id, egui::Sense::click_and_drag());
        let cursor = match direction {
            SplitDir::Horizontal => egui::CursorIcon::ResizeHorizontal,
            SplitDir::Vertical => egui::CursorIcon::ResizeVertical,
        };
        if resp.hovered() || resp.dragged() {
            ui.ctx().set_cursor_icon(cursor);
        }
        let th = crate::theme::theme();
        let painter = ui.painter();
        // rest = separator(--tasty-separator 파생 알파), hover/drag = accent_primary. (C-G7)
        let handle_color = if resp.hovered() || resp.dragged() {
            th.accent_primary().to_egui()
        } else {
            th.separator.to_egui_premultiplied()
        };
        // 시각적으로 얇은 가운데 선만 그린다 (handle_rect 자체는 hit-test용 두꺼운 영역).
        match direction {
            SplitDir::Horizontal => {
                let cx = handle_rect.center().x;
                painter.line_segment(
                    [
                        egui::pos2(cx, handle_rect.min.y),
                        egui::pos2(cx, handle_rect.max.y),
                    ],
                    egui::Stroke::new(1.0, handle_color),
                );
            }
            SplitDir::Vertical => {
                let cy = handle_rect.center().y;
                painter.line_segment(
                    [
                        egui::pos2(handle_rect.min.x, cy),
                        egui::pos2(handle_rect.max.x, cy),
                    ],
                    egui::Stroke::new(1.0, handle_color),
                );
            }
        }
        if resp.dragged()
            && let Some(ptr) = resp.interact_pointer_pos()
        {
            let coord = match direction {
                SplitDir::Horizontal => ptr.x,
                SplitDir::Vertical => ptr.y,
            };
            let raw_ratio = (coord - axis_min) / axis_size.max(1.0);
            // 양쪽 pane이 최소 MIN_PANE_PX 픽셀은 유지되도록 ratio를 clamp.
            let min_ratio = (MIN_PANE_PX / axis_size.max(1.0)).min(0.45);
            let max_ratio = 1.0 - min_ratio;
            let new_ratio = raw_ratio.clamp(min_ratio, max_ratio);
            let (mem_id, _) = splitter_memory_ids(sink, id_str);
            ui.ctx()
                .memory_mut(|m| m.data.insert_temp(mem_id, new_ratio));
            // 매 frame 송신은 부담이 클 수 있으나 plugin은 단순 ratio 저장만 하면 되므로
            // 실용상 문제 없음. 필요시 release 시점으로 throttle 가능.
            sink.push_event(UiEvent::SplitterDrag {
                node_id: id_str.to_string(),
                ratio: new_ratio,
            });
            ui.ctx().request_repaint();
        }
    }

    ui.advance_cursor_after_rect(avail);
}

/// Splitter의 egui memory 키 — 사용자가 조절한 ratio와 직전 protocol ratio를 분리 저장.
fn splitter_memory_ids(sink: &dyn UiSink, node_id: &str) -> (egui::Id, egui::Id) {
    let base = ("splitter_state", sink.salt(), node_id);
    let user = egui::Id::new(("user", base));
    let last_protocol = egui::Id::new(("last_protocol", base));
    (user, last_protocol)
}

/// Addressbar의 egui memory 키 — 사용자가 편집한 buffer 와 직전 protocol text 를 분리 저장.
fn addressbar_memory_ids(sink: &dyn UiSink, node_id: &str) -> (egui::Id, egui::Id) {
    let base = ("addressbar_state", sink.salt(), node_id);
    let user = egui::Id::new(("user", base));
    let last_protocol = egui::Id::new(("last_protocol", base));
    (user, last_protocol)
}

fn render_tree_node(
    ui: &mut Ui,
    tree_id: &str,
    n: &TreeNode,
    parent_path: &str,
    sink: &dyn UiSink,
) {
    let path = if parent_path.is_empty() {
        n.id.clone()
    } else {
        format!("{parent_path}/{}", n.id)
    };
    let label = match &n.icon {
        Some(icon) => format!("{icon} {}", n.label),
        None => n.label.clone(),
    };
    if !n.has_children && n.children.is_empty() {
        let resp = ui.selectable_label(n.selected, label);
        if resp.clicked() {
            sink.push_event(UiEvent::TreeSelect {
                node_id: tree_id.to_string(),
                selected: vec![path.clone()],
            });
        }
        if resp.double_clicked() {
            sink.push_event(UiEvent::TreeActivate {
                node_id: tree_id.to_string(),
                path: path.clone(),
            });
        }
    } else {
        let id_source = ui.make_persistent_id((tree_id, path.as_str()));
        let resp = egui::CollapsingHeader::new(label)
            .id_salt(id_source)
            .default_open(n.expanded)
            .show(ui, |ui| {
                for child in &n.children {
                    render_tree_node(ui, tree_id, child, &path, sink);
                }
            });
        if resp.header_response.clicked() {
            // 헤더 영역 클릭 자체로 selection 신호.
            sink.push_event(UiEvent::TreeSelect {
                node_id: tree_id.to_string(),
                selected: vec![path.clone()],
            });
        }
        if resp.header_response.double_clicked() {
            sink.push_event(UiEvent::TreeActivate {
                node_id: tree_id.to_string(),
                path: path.clone(),
            });
        }
        // 사용자가 펼침/접음 토글 시 plugin에 알림. 단, immediate-mode 한계로
        // 실제 internal state와 plugin 측 expanded 상태가 어긋날 수 있음.
        // 단계 06+에서 메모리화 개선.
        let now_open = resp.openness > 0.5;
        if now_open != n.expanded {
            sink.push_event(UiEvent::TreeExpand {
                node_id: tree_id.to_string(),
                path: path.clone(),
                expanded: now_open,
            });
        }
    }
}

/// DSL tone → 위젯 variant 매핑. `From` impl 은 두 타입이 모두 외부 크레이트라
/// orphan rule 에 막혀 호스트에서 불가 → 로컬 매핑 함수로 둔다.
fn tag_variant(tone: TagTone) -> tasty_ui_widgets::TagVariant {
    use tasty_ui_widgets::TagVariant as V;
    match tone {
        TagTone::Default => V::Default,
        TagTone::Accent => V::Accent,
        TagTone::Agent => V::Agent,
        TagTone::Success => V::Success,
        TagTone::Warning => V::Warning,
        TagTone::Danger => V::Danger,
    }
}

fn badge_variant(tone: BadgeTone) -> tasty_ui_widgets::BadgeVariant {
    use tasty_ui_widgets::BadgeVariant as V;
    match tone {
        BadgeTone::Danger => V::Danger,
        BadgeTone::Primary => V::Primary,
        BadgeTone::Agent => V::Agent,
        BadgeTone::Success => V::Success,
        BadgeTone::Neutral => V::Neutral,
    }
}

fn parse_color_token(token: &str) -> Option<egui::Color32> {
    if let Some(stripped) = token.strip_prefix('#')
        && stripped.len() == 6
    {
        let r = u8::from_str_radix(&stripped[0..2], 16).ok()?;
        let g = u8::from_str_radix(&stripped[2..4], 16).ok()?;
        let b = u8::from_str_radix(&stripped[4..6], 16).ok()?;
        // 외부 입력 (plugin 이 명시한 hex 색) — 정당한 dangerously 사용처.
        #[allow(clippy::disallowed_methods)]
        let color = egui::Color32::from_rgb(r, g, b);
        return Some(color);
    }
    let th = crate::theme::theme();
    Some(match token {
        "text" => th.text.to_egui(),
        "subtext1" => th.subtext1.to_egui(),
        "subtext0" => th.subtext0.to_egui(),
        "overlay0" => th.overlay0.to_egui(),
        "blue" => th.blue.to_egui(),
        "green" => th.green.to_egui(),
        "red" => th.red.to_egui(),
        "yellow" => th.yellow.to_egui(),
        _ => return None,
    })
}
