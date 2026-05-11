//! `UiNode` → egui 위젯 렌더링.
//!
//! 사용자 입력은 `RemoteSurface::pending_events`로 push 되어 다음 pump tick에 plugin에 송신.

use egui::Ui;

use crate::plugin::remote_surface::RemoteSurface;
use crate::plugin::ui_tree::{
    ButtonStyle, LabelStyle, SplitDir, TreeNode, UiEvent, UiNode,
};

pub fn render_remote_surface(ui: &mut Ui, surface: &RemoteSurface) {
    let tree_opt = surface.tree.lock().ok().and_then(|t| t.clone());
    match tree_opt {
        Some(node) => render_node(ui, &node, surface),
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
                        .color(egui::Color32::DARK_GRAY),
                );
            });
        }
    }
}

fn render_node(ui: &mut Ui, node: &UiNode, surface: &RemoteSurface) {
    match node {
        UiNode::Vbox { spacing, children } => {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = *spacing as f32;
                for (i, c) in children.iter().enumerate() {
                    // 자식별로 push_id로 id_salt를 분리 → 같은 종류의 stateful
                    // 위젯(ScrollArea, CollapsingHeader 등)이 형제 위치에 있어도
                    // egui ID가 충돌하지 않는다.
                    ui.push_id(i, |ui| render_node(ui, c, surface));
                }
            });
        }
        UiNode::Hbox { spacing, children } => {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = *spacing as f32;
                for (i, c) in children.iter().enumerate() {
                    ui.push_id(i, |ui| render_node(ui, c, surface));
                }
            });
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
                    render_node(ui, child, surface);
                });
        }
        UiNode::Splitter {
            direction,
            ratio,
            first,
            second,
            id,
        } => {
            render_splitter(ui, *direction, *ratio, id.as_deref(), first, second, surface);
        }
        UiNode::Label { text, style, color } => {
            let mut rt = egui::RichText::new(text);
            rt = match style {
                LabelStyle::Body => rt,
                LabelStyle::Caption => rt.small(),
                LabelStyle::Heading => rt.heading(),
                LabelStyle::Dim => rt.color(egui::Color32::GRAY),
            };
            if let Some(c) = color.as_deref() {
                if let Some(parsed) = parse_color_token(c) {
                    rt = rt.color(parsed);
                }
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
            tooltip_i18n_key,
        } => {
            let mut btn = egui::Button::new(label);
            if let ButtonStyle::Primary = style {
                btn = btn.fill(egui::Color32::from_rgb(48, 92, 222));
            }
            let resp = ui.add_enabled(*enabled, btn);
            let resp = match tooltip_i18n_key {
                Some(key) => resp.on_hover_text(crate::i18n::t(key)),
                None => resp,
            };
            if resp.clicked() {
                surface.push_event(UiEvent::Click {
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
                render_tree_node(ui, id, n, "", surface);
            }
        }
        UiNode::Addressbar {
            id,
            text,
            placeholder_i18n_key,
        } => {
            let mut buf = text.clone();
            let placeholder = placeholder_i18n_key
                .as_ref()
                .map(|k| crate::i18n::t(k))
                .unwrap_or_default();
            let resp = ui.add(
                egui::TextEdit::singleline(&mut buf)
                    .hint_text(placeholder)
                    .desired_width(f32::INFINITY),
            );
            if resp.changed() {
                surface.push_event(UiEvent::AddressbarChange {
                    node_id: id.clone(),
                    text: buf.clone(),
                });
            }
            if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                surface.push_event(UiEvent::AddressbarSubmit {
                    node_id: id.clone(),
                    text: buf,
                });
            }
        }
        UiNode::TextPreview {
            content,
            language: _,
        } => {
            // syntax highlighting은 향후 — 현 단계는 plain monospace.
            ui.add(egui::Label::new(egui::RichText::new(content).monospace()).wrap());
        }
        UiNode::Spacer { size } => {
            ui.add_space(*size as f32);
        }
    }
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
    surface: &RemoteSurface,
) {
    const HANDLE_THICKNESS: f32 = 6.0;
    const MIN_PANE_PX: f32 = 40.0;

    let avail = ui.available_rect_before_wrap();
    let effective_ratio = if let Some(id) = id {
        let (mem_id, last_protocol_id) = splitter_memory_ids(surface, id);
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
            let first_rect =
                egui::Rect::from_min_max(avail.min, egui::pos2(split_x, avail.max.y));
            let second_rect =
                egui::Rect::from_min_max(egui::pos2(split_x, avail.min.y), avail.max);
            let handle_rect = egui::Rect::from_min_max(
                egui::pos2(split_x - HANDLE_THICKNESS * 0.5, avail.min.y),
                egui::pos2(split_x + HANDLE_THICKNESS * 0.5, avail.max.y),
            );
            (first_rect, second_rect, handle_rect, avail.width(), avail.min.x)
        }
        SplitDir::Vertical => {
            let split_y = avail.min.y + avail.height() * r;
            let first_rect =
                egui::Rect::from_min_max(avail.min, egui::pos2(avail.max.x, split_y));
            let second_rect =
                egui::Rect::from_min_max(egui::pos2(avail.min.x, split_y), avail.max);
            let handle_rect = egui::Rect::from_min_max(
                egui::pos2(avail.min.x, split_y - HANDLE_THICKNESS * 0.5),
                egui::pos2(avail.max.x, split_y + HANDLE_THICKNESS * 0.5),
            );
            (first_rect, second_rect, handle_rect, avail.height(), avail.min.y)
        }
    };

    ui.scope_builder(egui::UiBuilder::new().max_rect(first_rect), |ui| {
        ui.push_id("split_first", |ui| render_node(ui, first, surface));
    });
    ui.scope_builder(egui::UiBuilder::new().max_rect(second_rect), |ui| {
        ui.push_id("split_second", |ui| render_node(ui, second, surface));
    });

    if let Some(id_str) = id {
        let handle_id = ui.make_persistent_id(("splitter_handle", surface.id, id_str));
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
        let handle_color = if resp.hovered() || resp.dragged() {
            th.blue
        } else {
            th.surface1
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
            let (mem_id, _) = splitter_memory_ids(surface, id_str);
            ui.ctx()
                .memory_mut(|m| m.data.insert_temp(mem_id, new_ratio));
            // 매 frame 송신은 부담이 클 수 있으나 plugin은 단순 ratio 저장만 하면 되므로
            // 실용상 문제 없음. 필요시 release 시점으로 throttle 가능.
            surface.push_event(UiEvent::SplitterDrag {
                node_id: id_str.to_string(),
                ratio: new_ratio,
            });
            ui.ctx().request_repaint();
        }
    }

    ui.advance_cursor_after_rect(avail);
}

/// Splitter의 egui memory 키 — 사용자가 조절한 ratio와 직전 protocol ratio를 분리 저장.
fn splitter_memory_ids(surface: &RemoteSurface, node_id: &str) -> (egui::Id, egui::Id) {
    let base = ("splitter_state", surface.id, node_id);
    let user = egui::Id::new(("user", base));
    let last_protocol = egui::Id::new(("last_protocol", base));
    (user, last_protocol)
}

fn render_tree_node(
    ui: &mut Ui,
    tree_id: &str,
    n: &TreeNode,
    parent_path: &str,
    surface: &RemoteSurface,
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
    if n.children.is_empty() {
        if ui.selectable_label(n.selected, label).clicked() {
            surface.push_event(UiEvent::TreeSelect {
                node_id: tree_id.to_string(),
                selected: vec![path.clone()],
            });
        }
    } else {
        let id_source = ui.make_persistent_id((tree_id, path.as_str()));
        let resp = egui::CollapsingHeader::new(label)
            .id_salt(id_source)
            .default_open(n.expanded)
            .show(ui, |ui| {
                for child in &n.children {
                    render_tree_node(ui, tree_id, child, &path, surface);
                }
            });
        if resp.header_response.clicked() {
            // 헤더 영역 클릭 자체로 selection 신호.
            surface.push_event(UiEvent::TreeSelect {
                node_id: tree_id.to_string(),
                selected: vec![path.clone()],
            });
        }
        // 사용자가 펼침/접음 토글 시 plugin에 알림. 단, immediate-mode 한계로
        // 실제 internal state와 plugin 측 expanded 상태가 어긋날 수 있음.
        // 단계 06+에서 메모리화 개선.
        let now_open = resp.openness > 0.5;
        if now_open != n.expanded {
            surface.push_event(UiEvent::TreeExpand {
                node_id: tree_id.to_string(),
                path: path.clone(),
                expanded: now_open,
            });
        }
    }
}

fn parse_color_token(token: &str) -> Option<egui::Color32> {
    if let Some(stripped) = token.strip_prefix('#') {
        if stripped.len() == 6 {
            let r = u8::from_str_radix(&stripped[0..2], 16).ok()?;
            let g = u8::from_str_radix(&stripped[2..4], 16).ok()?;
            let b = u8::from_str_radix(&stripped[4..6], 16).ok()?;
            return Some(egui::Color32::from_rgb(r, g, b));
        }
    }
    Some(match token {
        "text" => egui::Color32::from_rgb(205, 214, 244),
        "subtext1" => egui::Color32::from_rgb(186, 194, 222),
        "subtext0" => egui::Color32::from_rgb(166, 173, 200),
        "blue" => egui::Color32::from_rgb(137, 180, 250),
        "green" => egui::Color32::from_rgb(166, 227, 161),
        "red" => egui::Color32::from_rgb(243, 139, 168),
        "yellow" => egui::Color32::from_rgb(249, 226, 175),
        _ => return None,
    })
}
